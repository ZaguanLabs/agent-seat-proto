//! Private runtime directory and recoverable pathname socket ownership.

use std::env;
use std::fs::{self, DirBuilder, FileType};
use std::io;
use std::os::unix::fs::{
    DirBuilderExt as _, FileTypeExt as _, MetadataExt as _, PermissionsExt as _,
};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use agent_seat_proto::Advertisement;
use rustix::process::geteuid;

pub(crate) struct ListenerGuard {
    listener: UnixListener,
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl ListenerGuard {
    pub(crate) fn bind(explicit: Option<&Path>, screen: usize) -> Result<Self, String> {
        let path = match explicit {
            Some(path) => path.to_path_buf(),
            None => default_socket_path(screen)?,
        };
        validate_socket_path(&path)?;
        let parent = path
            .parent()
            .ok_or_else(|| "socket path has no parent directory".to_owned())?;
        ensure_private_directory(parent)?;

        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                recover_stale_socket(&path)?;
                UnixListener::bind(&path).map_err(|error| {
                    format!("cannot bind recovered socket {}: {error}", path.display())
                })?
            }
            Err(error) => {
                return Err(format!("cannot bind socket {}: {error}", path.display()));
            }
        };
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("cannot protect socket {}: {error}", path.display()))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot make listener nonblocking: {error}"))?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect bound socket {}: {error}", path.display()))?;
        Ok(Self {
            listener,
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    pub(crate) const fn listener(&self) -> &UnixListener {
        &self.listener
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|metadata| metadata.dev() == self.device && metadata.ino() == self.inode)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn default_socket_path(screen: usize) -> Result<PathBuf, String> {
    let base = env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| "XDG_RUNTIME_DIR is required unless --socket is provided".to_owned())?;
    let base = PathBuf::from(base);
    if !base.is_absolute() {
        return Err("XDG_RUNTIME_DIR must be absolute".to_owned());
    }
    let display = env::var_os("DISPLAY")
        .ok_or_else(|| "DISPLAY is required for the X11 provider".to_owned())?;
    let display_hash = fnv1a64(display.as_encoded_bytes());
    Ok(base
        .join("agent-seat")
        .join(format!("seat-{display_hash:016x}-s{screen}.sock")))
}

fn validate_socket_path(path: &Path) -> Result<(), String> {
    let text = path
        .to_str()
        .ok_or_else(|| "socket path must be UTF-8".to_owned())?;
    Advertisement::new(text)
        .map(|_| ())
        .map_err(|error| format!("invalid socket path: {error}"))
}

fn ensure_private_directory(path: &Path) -> Result<(), String> {
    if !path.exists() {
        let mut builder = DirBuilder::new();
        builder.recursive(false).mode(0o700);
        builder.create(path).map_err(|error| {
            format!(
                "cannot create runtime directory {}: {error}",
                path.display()
            )
        })?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "cannot inspect runtime directory {}: {error}",
            path.display()
        )
    })?;
    let uid = geteuid().as_raw();
    if !metadata.file_type().is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(format!(
            "runtime directory {} must be a private directory owned by UID {uid}",
            path.display()
        ));
    }
    Ok(())
}

fn recover_stale_socket(path: &Path) -> Result<(), String> {
    match UnixStream::connect(path) {
        Ok(_) => {
            return Err(format!(
                "a live provider socket already exists at {}",
                path.display()
            ));
        }
        Err(error)
            if !matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            return Err(format!(
                "cannot establish whether {} is stale: {error}",
                path.display()
            ));
        }
        Err(_) => {}
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect stale socket {}: {error}", path.display()))?;
    if !is_owned_socket(metadata.file_type(), metadata.uid()) {
        return Err(format!(
            "refusing to replace non-socket or foreign path {}",
            path.display()
        ));
    }
    fs::remove_file(path)
        .map_err(|error| format!("cannot remove stale socket {}: {error}", path.display()))
}

fn is_owned_socket(file_type: FileType, uid: u32) -> bool {
    file_type.is_socket() && uid == geteuid().as_raw()
}

const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        index += 1;
    }
    hash
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn private_dir() -> PathBuf {
        let path = env::temp_dir().join(format!(
            "agent-seat-t0-runtime-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = DirBuilder::new();
        builder.mode(0o700).create(&path).expect("private fixture");
        path
    }

    #[test]
    fn stale_owned_sockets_are_recovered_and_cleaned() {
        let directory = private_dir();
        let path = directory.join("seat.sock");
        drop(UnixListener::bind(&path).expect("stale socket fixture"));
        let guard = ListenerGuard::bind(Some(&path), 0).expect("recover stale socket");
        assert!(path.exists());
        drop(guard);
        assert!(!path.exists());
        fs::remove_dir(directory).expect("remove fixture directory");
    }

    #[test]
    fn existing_non_socket_is_never_replaced() {
        let directory = private_dir();
        let path = directory.join("seat.sock");
        fs::write(&path, b"not a socket").expect("file fixture");
        assert!(ListenerGuard::bind(Some(&path), 0).is_err());
        assert_eq!(fs::read(&path).expect("preserved fixture"), b"not a socket");
        fs::remove_file(path).expect("remove fixture file");
        fs::remove_dir(directory).expect("remove fixture directory");
    }
}
