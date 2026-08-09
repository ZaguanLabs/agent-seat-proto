//! Safe, strict adoption of systemd-owned file descriptors.

use std::env;
use std::fmt;
use std::os::fd::OwnedFd;

use rustix::io::{FdFlags, fcntl_getfd, fcntl_setfd};

/// One named file descriptor transferred by the service manager.
#[derive(Debug)]
pub struct InheritedFile {
    name: String,
    descriptor: OwnedFd,
}

impl InheritedFile {
    /// Returns the manager-provided descriptor name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Transfers ownership of the descriptor to its validated consumer.
    pub fn into_descriptor(self) -> OwnedFd {
        self.descriptor
    }
}

/// Failure to adopt an exact named descriptor set from systemd.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InheritedFileError {
    /// No descriptor set was addressed to this process.
    Missing,
    /// The activation environment or descriptor names were malformed.
    Malformed,
    /// The close-on-exec flag could not be applied.
    CannotConfine,
}

impl fmt::Display for InheritedFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Missing => "no inherited descriptor set was addressed to this process",
            Self::Malformed => "the inherited descriptor environment is malformed",
            Self::CannotConfine => "an inherited descriptor could not be confined",
        })
    }
}

impl std::error::Error for InheritedFileError {}

/// Adopts the exact named descriptors addressed to the current process.
///
/// This validates the systemd PID/count/name environment, requires one
/// non-empty name for every active descriptor and already-consumed standard
/// stream, and applies close-on-exec before returning safe owned descriptors.
/// The caller must still validate the exact expected names, order, file types,
/// and contents.
pub fn receive_inherited_files(
    maximum_count: usize,
    consumed_name_count: usize,
) -> Result<Vec<InheritedFile>, InheritedFileError> {
    let announced_count = env::var("LISTEN_FDS")
        .map_err(|_| InheritedFileError::Missing)?
        .parse::<usize>()
        .map_err(|_| InheritedFileError::Malformed)?;
    if announced_count == 0 || announced_count > maximum_count {
        return Err(InheritedFileError::Malformed);
    }
    let announced_names = env::var("LISTEN_FDNAMES").map_err(|_| InheritedFileError::Malformed)?;
    let total_name_count = announced_count
        .checked_add(consumed_name_count)
        .ok_or(InheritedFileError::Malformed)?;
    let names = validate_names(&announced_names, total_name_count)?;
    let active_names = names.into_iter().skip(consumed_name_count);
    let received = sd_listen_fds::get().map_err(|_| InheritedFileError::Malformed)?;
    if received.len() != announced_count {
        return Err(InheritedFileError::Missing);
    }

    received
        .into_iter()
        .zip(active_names)
        .map(|((_shifted_name, descriptor), expected_name)| {
            let descriptor = descriptor.into_std();
            let flags = fcntl_getfd(&descriptor).map_err(|_| InheritedFileError::CannotConfine)?;
            fcntl_setfd(&descriptor, flags | FdFlags::CLOEXEC)
                .map_err(|_| InheritedFileError::CannotConfine)?;
            Ok(InheritedFile {
                name: expected_name.to_owned(),
                descriptor,
            })
        })
        .collect()
}

fn validate_names(source: &str, count: usize) -> Result<Vec<&str>, InheritedFileError> {
    if count == 0 {
        return Err(InheritedFileError::Missing);
    }
    let names = source.split(':').collect::<Vec<_>>();
    if names.len() != count
        || names.iter().any(|name| {
            name.is_empty() || name.len() > 255 || name.bytes().any(|byte| byte.is_ascii_control())
        })
    {
        return Err(InheritedFileError::Malformed);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_names_are_exact_nonempty_and_bounded() {
        assert_eq!(
            validate_names("manifest:event0", 2),
            Ok(vec!["manifest", "event0"])
        );
        for (source, count) in [
            ("", 0),
            ("", 1),
            ("manifest", 2),
            ("manifest:event0:extra", 2),
            ("manifest::event0", 3),
            ("manifest:\nevent0", 2),
        ] {
            assert!(validate_names(source, count).is_err());
        }
        assert!(validate_names(&"x".repeat(256), 1).is_err());
    }
}
