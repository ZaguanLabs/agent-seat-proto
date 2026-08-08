//! Four-byte length-prefixed strict JSON framing.

use std::fmt;
use std::io::{self, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Validate;

const PREFIX_BYTES: usize = size_of::<u32>();

/// Result of reading one frame or a clean stream end between frames.
#[derive(Debug, Eq, PartialEq)]
pub enum ReadFrame<T> {
    /// One complete, decoded, validated message.
    Message(T),
    /// EOF occurred before any byte of the next prefix.
    CleanEof,
}

/// A bounded framing, JSON, or message-validation failure.
#[derive(Debug)]
pub enum CodecError {
    /// The underlying stream failed.
    Io(io::Error),
    /// EOF split a length prefix.
    TruncatedPrefix,
    /// EOF split a payload.
    TruncatedPayload,
    /// A zero-length frame is invalid.
    Empty,
    /// The declared or encoded payload exceeds the direction's bound.
    TooLarge {
        /// Declared or encoded payload bytes.
        length: usize,
        /// Direction-specific maximum bytes.
        limit: usize,
    },
    /// JSON syntax or schema validation failed.
    Json(serde_json::Error),
    /// A cross-field protocol invariant failed.
    Invalid(&'static str),
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "stream I/O failed: {error}"),
            Self::TruncatedPrefix => formatter.write_str("frame length prefix was truncated"),
            Self::TruncatedPayload => formatter.write_str("frame payload was truncated"),
            Self::Empty => formatter.write_str("frame payload is empty"),
            Self::TooLarge { length, limit } => {
                write!(formatter, "frame has {length} bytes; maximum is {limit}")
            }
            Self::Json(error) => write!(formatter, "frame JSON is invalid: {error}"),
            Self::Invalid(error) => write!(formatter, "frame message is invalid: {error}"),
        }
    }
}

impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

/// Reads one bounded frame, refusing its size before payload allocation.
///
/// # Errors
///
/// Returns [`CodecError`] for I/O failure, truncation, an invalid length,
/// malformed/unknown JSON, or failed message validation.
pub fn read_frame<R, T>(reader: &mut R, limit: usize) -> Result<ReadFrame<T>, CodecError>
where
    R: Read,
    T: DeserializeOwned + Validate,
{
    let mut prefix = [0_u8; PREFIX_BYTES];
    let mut read = 0;
    while read < prefix.len() {
        match reader.read(&mut prefix[read..]) {
            Ok(0) if read == 0 => return Ok(ReadFrame::CleanEof),
            Ok(0) => return Err(CodecError::TruncatedPrefix),
            Ok(count) => read += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(CodecError::Io(error)),
        }
    }
    let length = usize::try_from(u32::from_be_bytes(prefix)).expect("u32 fits usize");
    if length == 0 {
        return Err(CodecError::Empty);
    }
    if length > limit {
        return Err(CodecError::TooLarge { length, limit });
    }
    let mut payload = vec![0_u8; length];
    if let Err(error) = reader.read_exact(&mut payload) {
        return if error.kind() == io::ErrorKind::UnexpectedEof {
            Err(CodecError::TruncatedPayload)
        } else {
            Err(CodecError::Io(error))
        };
    }
    let message: T = serde_json::from_slice(&payload).map_err(CodecError::Json)?;
    message.validate().map_err(CodecError::Invalid)?;
    Ok(ReadFrame::Message(message))
}

/// Serializes, validates, bounds, and writes one frame.
///
/// # Errors
///
/// Returns [`CodecError`] for message validation, JSON encoding, an oversized
/// payload, or stream failure.
pub fn write_frame<W, T>(writer: &mut W, message: &T, limit: usize) -> Result<(), CodecError>
where
    W: Write,
    T: Serialize + Validate,
{
    message.validate().map_err(CodecError::Invalid)?;
    let payload = serde_json::to_vec(message).map_err(CodecError::Json)?;
    if payload.is_empty() {
        return Err(CodecError::Empty);
    }
    if payload.len() > limit || payload.len() > u32::MAX as usize {
        return Err(CodecError::TooLarge {
            length: payload.len(),
            limit,
        });
    }
    writer
        .write_all(&(payload.len() as u32).to_be_bytes())
        .and_then(|()| writer.write_all(&payload))
        .map_err(CodecError::Io)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Number {
        value: u8,
    }

    impl Validate for Number {
        fn validate(&self) -> Result<(), &'static str> {
            (self.value != 0)
                .then_some(())
                .ok_or("value must be nonzero")
        }
    }

    #[test]
    fn frames_round_trip() {
        let expected = Number { value: 7 };
        let mut encoded = Vec::new();
        write_frame(&mut encoded, &expected, 64).expect("encode");
        assert_eq!(
            read_frame(&mut encoded.as_slice(), 64).expect("decode"),
            ReadFrame::Message(expected)
        );
    }

    #[test]
    fn oversized_prefix_is_refused_before_payload_read() {
        let input = 65_u32.to_be_bytes();
        assert!(matches!(
            read_frame::<_, Number>(&mut input.as_slice(), 64),
            Err(CodecError::TooLarge {
                length: 65,
                limit: 64
            })
        ));
    }

    #[test]
    fn clean_close_and_both_truncations_are_distinct() {
        assert!(matches!(
            read_frame::<_, Number>(&mut [].as_slice(), 64),
            Ok(ReadFrame::CleanEof)
        ));
        assert!(matches!(
            read_frame::<_, Number>(&mut [0, 0].as_slice(), 64),
            Err(CodecError::TruncatedPrefix)
        ));
        assert!(matches!(
            read_frame::<_, Number>(&mut [0, 0, 0, 2, b'{'].as_slice(), 64),
            Err(CodecError::TruncatedPayload)
        ));
    }

    #[test]
    fn unknown_fields_and_invalid_values_fail() {
        for payload in [br#"{"value":1,"extra":2}"#.as_slice(), br#"{"value":0}"#] {
            let mut input = Vec::from((payload.len() as u32).to_be_bytes());
            input.extend_from_slice(payload);
            assert!(read_frame::<_, Number>(&mut input.as_slice(), 64).is_err());
        }
    }

    #[test]
    fn duplicate_fields_are_rejected() {
        let payload = br#"{"value":1,"value":2}"#;
        let mut input = Vec::from((payload.len() as u32).to_be_bytes());
        input.extend_from_slice(payload);
        assert!(matches!(
            read_frame::<_, Number>(&mut input.as_slice(), 64),
            Err(CodecError::Json(_))
        ));
    }

    #[test]
    fn outbound_validation_precedes_writes() {
        let mut encoded = Vec::new();
        assert!(matches!(
            write_frame(&mut encoded, &Number { value: 0 }, 64),
            Err(CodecError::Invalid("value must be nonzero"))
        ));
        assert!(encoded.is_empty());

        assert!(matches!(
            write_frame(&mut encoded, &Number { value: 1 }, 1),
            Err(CodecError::TooLarge { limit: 1, .. })
        ));
        assert!(encoded.is_empty());
    }
}
