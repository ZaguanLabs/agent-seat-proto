//! Allocation-free parsing of the X11 discovery property.

use std::fmt;
use std::fmt::Write as _;

use crate::{MAX_ADVERTISEMENT_BYTES, PROTOCOL_NAME, PROTOCOL_REVISION};

const SEPARATOR: char = '\0';
const REVISION_TEXT: &str = "3";

/// A validated Agent Seat advertisement borrowed from its source value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Advertisement<'a> {
    socket: &'a str,
}

impl<'a> Advertisement<'a> {
    /// Parses the exact revision-3 advertisement grammar without allocation.
    ///
    /// # Errors
    ///
    /// Returns [`AdvertisementError`] for an oversized value, invalid field
    /// count, non-canonical protocol/revision, or invalid socket path.
    pub fn parse(value: &'a str) -> Result<Self, AdvertisementError> {
        if value.len() > MAX_ADVERTISEMENT_BYTES {
            return Err(AdvertisementError::TooLarge);
        }
        let mut fields = value.split(SEPARATOR);
        if fields.next() != Some(PROTOCOL_NAME) {
            return Err(AdvertisementError::Protocol);
        }
        let revision = fields.next().ok_or(AdvertisementError::Fields)?;
        if revision != REVISION_TEXT {
            return Err(AdvertisementError::Revision);
        }
        let socket = fields.next().ok_or(AdvertisementError::Fields)?;
        if fields.next().is_some() {
            return Err(AdvertisementError::Fields);
        }
        validate_socket(socket)?;
        Ok(Self { socket })
    }

    /// Builds a validated advertisement around `socket`.
    ///
    /// # Errors
    ///
    /// Returns [`AdvertisementError`] when the socket is not a bounded
    /// absolute pathname or the complete encoding would exceed 256 bytes.
    pub fn new(socket: &'a str) -> Result<Self, AdvertisementError> {
        validate_socket(socket)?;
        let encoded_len = PROTOCOL_NAME.len() + 1 + REVISION_TEXT.len() + 1 + socket.len();
        if encoded_len > MAX_ADVERTISEMENT_BYTES {
            return Err(AdvertisementError::TooLarge);
        }
        Ok(Self { socket })
    }

    /// Returns the absolute pathname socket field.
    #[must_use]
    pub const fn socket(self) -> &'a str {
        self.socket
    }

    /// Encodes the canonical three-field value.
    #[must_use]
    pub fn encode(self) -> String {
        let mut encoded = String::with_capacity(
            PROTOCOL_NAME.len() + 1 + REVISION_TEXT.len() + 1 + self.socket.len(),
        );
        write!(
            encoded,
            "{PROTOCOL_NAME}{SEPARATOR}{PROTOCOL_REVISION}{SEPARATOR}{}",
            self.socket
        )
        .expect("writing to a String cannot fail");
        encoded
    }
}

fn validate_socket(socket: &str) -> Result<(), AdvertisementError> {
    if socket.is_empty() || !socket.starts_with('/') || socket.contains(SEPARATOR) {
        return Err(AdvertisementError::Socket);
    }
    // Linux sockaddr_un reserves one byte for the terminating NUL.
    if socket.len() > 107 {
        return Err(AdvertisementError::Socket);
    }
    Ok(())
}

/// Why an X11 discovery value was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdvertisementError {
    /// The complete value exceeds the public bound.
    TooLarge,
    /// The field count is not exactly three.
    Fields,
    /// The protocol name differs.
    Protocol,
    /// The revision is unsupported or non-canonical.
    Revision,
    /// The socket is empty, relative, or too long.
    Socket,
}

impl fmt::Display for AdvertisementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooLarge => "advertisement exceeds 256 bytes",
            Self::Fields => "advertisement must contain exactly three fields",
            Self::Protocol => "advertisement names another protocol",
            Self::Revision => "advertisement names an unsupported revision",
            Self::Socket => "advertisement socket is not a bounded absolute path",
        })
    }
}

impl std::error::Error for AdvertisementError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_advertisements_round_trip() {
        let advertisement =
            Advertisement::new("/run/user/1000/agent seat.sock").expect("valid absolute path");
        let encoded = advertisement.encode();
        assert_eq!(Advertisement::parse(&encoded), Ok(advertisement));
    }

    #[test]
    fn malformed_and_noncanonical_values_are_refused() {
        for value in [
            "",
            "agent-seat",
            "agent-seat\x003",
            "agent-seat\x0003\0/tmp/s",
            "agent-seat\x00+3\0/tmp/s",
            "agent-seat\x003\0relative",
            "agent-seat\x003\0/tmp/s\0extra",
        ] {
            assert!(Advertisement::parse(value).is_err(), "accepted {value:?}");
        }
    }
}
