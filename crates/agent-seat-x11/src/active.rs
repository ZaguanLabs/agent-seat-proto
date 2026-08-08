//! Private, lock-held evidence of the policy loaded by a running provider.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, Mode, OFlags};
use rustix::io::Errno;
use rustix::process::geteuid;

use crate::config::{MAX_CONFIG_BYTES, PolicySnapshot};

const MAGIC: &[u8; 16] = b"agent-seat-act1\n";
const HEADER_BYTES: usize = MAGIC.len() + 12;
const MAX_POLICY_PATH_BYTES: usize = 4_096;
const MAX_MARKER_BYTES: usize = HEADER_BYTES + MAX_POLICY_PATH_BYTES + MAX_CONFIG_BYTES as usize;
const MAX_MARKERS: usize = 128;

/// Best-effort evidence about policies loaded by current provider processes.
///
/// Evidence is held live by an advisory lock in the private XDG runtime
/// directory. It guides restart instructions but grants no authority and is
/// not a same-user security boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivePolicyStatus {
    /// No locked record for this policy path was found.
    NotReported,
    /// One provider reports the exact currently saved policy.
    Matching {
        /// Provider process ID recorded at startup.
        pid: u32,
    },
    /// One provider reports a different policy for the same saved path.
    Different {
        /// Provider process ID recorded at startup.
        pid: u32,
    },
    /// More than one provider reports the same policy path.
    Multiple {
        /// Number of locked records found within the published bound.
        count: usize,
        /// Whether every reported policy matches the current saved source.
        all_match: bool,
    },
    /// XDG runtime discovery is unavailable in this process environment.
    Unavailable,
}

/// Reads lock-held active-policy evidence without connecting to X11 or a
/// provider socket.
///
/// # Errors
///
/// Returns an error for an unsafe runtime directory or marker, excessive
/// marker count, malformed locked evidence, or an I/O failure that prevents an
/// honest status result.
pub fn active_policy_status(snapshot: &PolicySnapshot) -> Result<ActivePolicyStatus, String> {
    let Some(directory) = state_directory()? else {
        return Ok(ActivePolicyStatus::Unavailable);
    };
    match fs::symlink_metadata(&directory) {
        Ok(_) => crate::runtime::validate_private_directory(&directory)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ActivePolicyStatus::NotReported);
        }
        Err(error) => {
            return Err(format!(
                "cannot inspect active-policy directory {}: {error}",
                directory.display()
            ));
        }
    }

    let mut records = Vec::new();
    let entries = fs::read_dir(&directory).map_err(|error| {
        format!(
            "cannot read active-policy directory {}: {error}",
            directory.display()
        )
    })?;
    let mut scanned = 0_usize;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read active-policy entry: {error}"))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with("active-") || !name.ends_with(".policy") {
            continue;
        }
        scanned += 1;
        if scanned > MAX_MARKERS {
            return Err(format!(
                "active-policy marker count exceeds the {MAX_MARKERS}-file bound"
            ));
        }
        if let Some(record) = read_locked_marker(&entry.path())? {
            if record.path == snapshot.path().as_os_str().as_encoded_bytes() {
                records.push((record.pid, record.source == snapshot.source().as_bytes()));
            }
        }
    }
    match records.as_slice() {
        [] => Ok(ActivePolicyStatus::NotReported),
        [(pid, true)] => Ok(ActivePolicyStatus::Matching { pid: *pid }),
        [(pid, false)] => Ok(ActivePolicyStatus::Different { pid: *pid }),
        records => Ok(ActivePolicyStatus::Multiple {
            count: records.len(),
            all_match: records.iter().all(|(_, matches)| *matches),
        }),
    }
}

pub(crate) struct ActivePolicyGuard {
    file: File,
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl ActivePolicyGuard {
    pub(crate) fn publish(snapshot: &PolicySnapshot) -> Result<Self, String> {
        let directory = state_directory()?.ok_or_else(|| {
            "XDG_RUNTIME_DIR is unavailable for active-policy reporting".to_owned()
        })?;
        crate::runtime::ensure_private_directory(&directory)?;
        let path = directory.join(format!("active-{}.policy", std::process::id()));
        if path.exists() {
            remove_stale_marker(&path)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true).mode(0o600);
        let mut file = options
            .open(&path)
            .map_err(|error| format!("cannot create active-policy marker: {error}"))?;
        let prepared = (|| {
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("cannot secure active-policy marker: {error}"))?;
            rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive)
                .map_err(|error| format!("cannot lock active-policy marker: {error}"))?;
            let body = marker_body(snapshot)?;
            file.write_all(&body)
                .and_then(|()| file.sync_all())
                .map_err(|error| format!("cannot write active-policy marker: {error}"))?;
            file.metadata()
                .map_err(|error| format!("cannot inspect active-policy marker: {error}"))
        })();
        let metadata = match prepared {
            Ok(metadata) => metadata,
            Err(error) => {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(error);
            }
        };
        Ok(Self {
            file,
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

impl Drop for ActivePolicyGuard {
    fn drop(&mut self) {
        let _ = self.file.sync_all();
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|metadata| metadata.dev() == self.device && metadata.ino() == self.inode)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct Marker {
    pid: u32,
    path: Vec<u8>,
    source: Vec<u8>,
}

fn state_directory() -> Result<Option<PathBuf>, String> {
    let Some(base) = env::var_os("XDG_RUNTIME_DIR") else {
        return Ok(None);
    };
    let base = PathBuf::from(base);
    if !base.is_absolute() {
        return Err("XDG_RUNTIME_DIR must be absolute".to_owned());
    }
    Ok(Some(base.join("agent-seat")))
}

fn marker_body(snapshot: &PolicySnapshot) -> Result<Vec<u8>, String> {
    let path = snapshot.path().as_os_str().as_encoded_bytes();
    if path.len() > MAX_POLICY_PATH_BYTES {
        return Err(format!(
            "policy path exceeds the {MAX_POLICY_PATH_BYTES}-byte active-state bound"
        ));
    }
    let path_len = u32::try_from(path.len())
        .map_err(|_| "policy path length cannot be represented".to_owned())?;
    let source_len = u32::try_from(snapshot.source().len())
        .map_err(|_| "policy source length cannot be represented".to_owned())?;
    let mut body = Vec::with_capacity(HEADER_BYTES + path.len() + snapshot.source().len());
    body.extend_from_slice(MAGIC);
    body.extend_from_slice(&std::process::id().to_le_bytes());
    body.extend_from_slice(&path_len.to_le_bytes());
    body.extend_from_slice(&source_len.to_le_bytes());
    body.extend_from_slice(path);
    body.extend_from_slice(snapshot.source().as_bytes());
    Ok(body)
}

fn read_locked_marker(path: &Path) -> Result<Option<Marker>, String> {
    let descriptor = match rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(descriptor) => descriptor,
        Err(Errno::NOENT) => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot open active-policy marker {}: {error}",
                path.display()
            ));
        }
    };
    let file = File::from(descriptor);
    validate_marker_file(path, &file)?;
    match rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => return Ok(None),
        Err(Errno::AGAIN) => {}
        Err(error) => {
            return Err(format!(
                "cannot inspect active-policy lock {}: {error}",
                path.display()
            ));
        }
    }
    read_marker(path, file).map(Some)
}

fn validate_marker_file(path: &Path, file: &File) -> Result<(), String> {
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    let uid = geteuid().as_raw();
    if !metadata.file_type().is_file() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(format!(
            "active-policy marker {} must be a private regular file owned by UID {uid}",
            path.display()
        ));
    }
    if metadata.len() > MAX_MARKER_BYTES as u64 {
        return Err(format!(
            "active-policy marker {} exceeds its byte bound",
            path.display()
        ));
    }
    Ok(())
}

fn read_marker(path: &Path, file: File) -> Result<Marker, String> {
    let mut bytes = Vec::new();
    file.take(MAX_MARKER_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.len() > MAX_MARKER_BYTES
        || bytes.len() < HEADER_BYTES
        || &bytes[..MAGIC.len()] != MAGIC
    {
        return Err(format!(
            "active-policy marker {} is malformed",
            path.display()
        ));
    }
    let pid = read_u32(&bytes, MAGIC.len())?;
    let path_len = read_u32(&bytes, MAGIC.len() + 4)? as usize;
    let source_len = read_u32(&bytes, MAGIC.len() + 8)? as usize;
    let expected = HEADER_BYTES
        .checked_add(path_len)
        .and_then(|length| length.checked_add(source_len))
        .ok_or_else(|| {
            format!(
                "active-policy marker {} has invalid lengths",
                path.display()
            )
        })?;
    if path_len > MAX_POLICY_PATH_BYTES
        || source_len > MAX_CONFIG_BYTES as usize
        || expected != bytes.len()
    {
        return Err(format!(
            "active-policy marker {} has invalid lengths",
            path.display()
        ));
    }
    let path_end = HEADER_BYTES + path_len;
    Ok(Marker {
        pid,
        path: bytes[HEADER_BYTES..path_end].to_vec(),
        source: bytes[path_end..].to_vec(),
    })
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "active-policy marker header is truncated".to_owned())?;
    let value: [u8; 4] = value
        .try_into()
        .map_err(|_| "active-policy marker integer is malformed".to_owned())?;
    Ok(u32::from_le_bytes(value))
}

fn remove_stale_marker(path: &Path) -> Result<(), String> {
    let descriptor = rustix::fs::open(
        path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| format!("cannot open prior active-policy marker: {error}"))?;
    let file = File::from(descriptor);
    validate_marker_file(path, &file)?;
    rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive).map_err(|error| {
        format!("prior active-policy marker is still locked by a provider: {error}")
    })?;
    let open = file
        .metadata()
        .map_err(|error| format!("cannot inspect prior active-policy marker: {error}"))?;
    let current = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect prior active-policy path: {error}"))?;
    if open.dev() != current.dev() || open.ino() != current.ino() {
        return Err("prior active-policy marker changed during recovery".to_owned());
    }
    fs::remove_file(path)
        .map_err(|error| format!("cannot remove stale active-policy marker: {error}"))
}
