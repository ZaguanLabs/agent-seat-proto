//! Bounded deployment metadata for the complete kernel input-event class map.

use std::fmt;
use std::path::{Component, Path, PathBuf};

const HEADER: &[u8] = b"ASIS\t1\n";

/// Maximum number of `/sys/class/input/event*` mappings in one manifest.
pub const MAX_INPUT_CLASS_MAPPINGS: usize = 256;

/// Maximum encoded initial-input-set manifest size.
pub const MAX_INPUT_SET_BYTES: usize = 64 * 1024;

/// One exact event-number to canonical-sysfs-path mapping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputClassMapping {
    event_number: u32,
    sysfs_path: PathBuf,
}

impl InputClassMapping {
    /// Constructs one validated mapping.
    ///
    /// # Errors
    ///
    /// Returns [`InputSetError::Malformed`] unless the path is a normalized,
    /// UTF-8 `/sys/devices/.../eventN` path matching `event_number`.
    pub fn new(event_number: u32, sysfs_path: PathBuf) -> Result<Self, InputSetError> {
        if !valid_sysfs_path(event_number, &sysfs_path) {
            return Err(InputSetError::Malformed);
        }
        Ok(Self {
            event_number,
            sysfs_path,
        })
    }

    /// Returns the canonical kernel input-event class number.
    #[must_use]
    pub const fn event_number(&self) -> u32 {
        self.event_number
    }

    /// Returns the canonical sysfs target path.
    #[must_use]
    pub fn sysfs_path(&self) -> &Path {
        &self.sysfs_path
    }
}

/// Strict initial-input-set manifest failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputSetError {
    /// The manifest or one mapping is noncanonical or internally inconsistent.
    Malformed,
    /// The manifest exceeds its byte or event-count bound.
    BoundExceeded,
}

impl fmt::Display for InputSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => formatter.write_str("initial input set is malformed"),
            Self::BoundExceeded => formatter.write_str("initial input set exceeds its bound"),
        }
    }
}

impl std::error::Error for InputSetError {}

/// Encodes one sorted, nonempty, exact class mapping.
///
/// # Errors
///
/// Returns [`InputSetError`] for an invalid, duplicate, unsorted, empty, or
/// over-bound mapping set.
pub fn encode_input_set(mappings: &[InputClassMapping]) -> Result<Vec<u8>, InputSetError> {
    validate_order(mappings)?;
    let mut output = Vec::with_capacity(HEADER.len().saturating_add(mappings.len() * 64));
    output.extend_from_slice(HEADER);
    for mapping in mappings {
        let path = mapping
            .sysfs_path
            .to_str()
            .ok_or(InputSetError::Malformed)?;
        output.extend_from_slice(format!("event{}\t{path}\n", mapping.event_number).as_bytes());
        if output.len() > MAX_INPUT_SET_BYTES {
            return Err(InputSetError::BoundExceeded);
        }
    }
    Ok(output)
}

/// Parses one strict, bounded initial-input-set manifest.
///
/// # Errors
///
/// Returns [`InputSetError`] for unknown revisions, malformed paths, duplicate
/// or unsorted entries, trailing data, or exceeded bounds.
pub fn decode_input_set(source: &[u8]) -> Result<Vec<InputClassMapping>, InputSetError> {
    if source.len() > MAX_INPUT_SET_BYTES {
        return Err(InputSetError::BoundExceeded);
    }
    if !source.ends_with(b"\n") {
        return Err(InputSetError::Malformed);
    }
    let source = std::str::from_utf8(source).map_err(|_| InputSetError::Malformed)?;
    let mut lines = source.split_terminator('\n');
    if lines.next() != Some("ASIS\t1") {
        return Err(InputSetError::Malformed);
    }
    let mut mappings = Vec::new();
    for line in lines {
        if mappings.len() >= MAX_INPUT_CLASS_MAPPINGS {
            return Err(InputSetError::BoundExceeded);
        }
        let (event, path) = line.split_once('\t').ok_or(InputSetError::Malformed)?;
        if path.contains('\t') {
            return Err(InputSetError::Malformed);
        }
        let number = parse_event_name(event).ok_or(InputSetError::Malformed)?;
        mappings.push(InputClassMapping::new(number, PathBuf::from(path))?);
    }
    validate_order(&mappings)?;
    Ok(mappings)
}

fn validate_order(mappings: &[InputClassMapping]) -> Result<(), InputSetError> {
    if mappings.is_empty() || mappings.len() > MAX_INPUT_CLASS_MAPPINGS {
        return Err(if mappings.len() > MAX_INPUT_CLASS_MAPPINGS {
            InputSetError::BoundExceeded
        } else {
            InputSetError::Malformed
        });
    }
    let mut previous = None;
    for mapping in mappings {
        if !valid_sysfs_path(mapping.event_number, &mapping.sysfs_path)
            || previous.is_some_and(|number| number >= mapping.event_number)
        {
            return Err(InputSetError::Malformed);
        }
        previous = Some(mapping.event_number);
    }
    Ok(())
}

fn valid_sysfs_path(event_number: u32, path: &Path) -> bool {
    let Some(path_text) = path.to_str() else {
        return false;
    };
    if path_text.as_bytes().contains(&b'\0')
        || path_text.contains(['\n', '\r', '\t'])
        || !path.starts_with("/sys/devices")
        || path.file_name().and_then(|name| name.to_str())
            != Some(format!("event{event_number}").as_str())
    {
        return false;
    }
    path.components()
        .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

fn parse_event_name(value: &str) -> Option<u32> {
    let number = value.strip_prefix("event")?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = number.parse::<u32>().ok()?;
    (value == format!("event{number}")).then_some(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(number: u32) -> InputClassMapping {
        InputClassMapping::new(
            number,
            PathBuf::from(format!(
                "/sys/devices/platform/example/input/input{number}/event{number}"
            )),
        )
        .expect("valid mapping fixture")
    }

    #[test]
    fn initial_input_sets_round_trip_canonically() {
        let mappings = [mapping(2), mapping(10)];
        let encoded = encode_input_set(&mappings).expect("encode mapping fixture");
        assert_eq!(decode_input_set(&encoded), Ok(mappings.to_vec()));
        assert_eq!(
            encoded,
            b"ASIS\t1\nevent2\t/sys/devices/platform/example/input/input2/event2\nevent10\t/sys/devices/platform/example/input/input10/event10\n"
        );
    }

    #[test]
    fn initial_input_sets_reject_ambiguity_and_bounds() {
        assert_eq!(encode_input_set(&[]), Err(InputSetError::Malformed));
        assert_eq!(
            encode_input_set(&[mapping(2), mapping(2)]),
            Err(InputSetError::Malformed)
        );
        assert_eq!(
            encode_input_set(&[mapping(10), mapping(2)]),
            Err(InputSetError::Malformed)
        );
        for malformed in [
            &b"ASIS\t2\nevent2\t/sys/devices/example/event2\n"[..],
            &b"ASIS\t1\nevent02\t/sys/devices/example/event2\n"[..],
            &b"ASIS\t1\nevent2\t/sys/devices/example/event3\n"[..],
            &b"ASIS\t1\nevent2\t/sys/devices/../example/event2\n"[..],
            &b"ASIS\t1\nevent2\t/sys/devices/example/event2"[..],
            &b"ASIS\t1\n"[..],
        ] {
            assert_eq!(decode_input_set(malformed), Err(InputSetError::Malformed));
        }
        assert_eq!(
            decode_input_set(&vec![b'x'; MAX_INPUT_SET_BYTES + 1]),
            Err(InputSetError::BoundExceeded)
        );
    }
}
