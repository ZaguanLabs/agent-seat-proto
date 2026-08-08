//! Exact socket-source precedence and X11 ownership validation.

use std::fmt;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use agent_seat_proto::{
    ADVERTISEMENT_PROPERTY, Advertisement, AdvertisementError, MAX_ADVERTISEMENT_BYTES,
};
use x11rb::NONE;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

const MAX_SOCKET_PATH_BYTES: usize = 107;
const MAX_PROPERTY_LONGS: u32 = (MAX_ADVERTISEMENT_BYTES as u32).div_ceil(4) + 1;

/// Selected socket source was unusable.
#[derive(Debug)]
pub(crate) enum DiscoveryError {
    InvalidPath(&'static str),
    X11(String),
    Advertisement(AdvertisementError),
    AdvertisementShape(&'static str),
    AdvertisementUtf8,
}

impl DiscoveryError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::InvalidPath(_) => "invalid_argument",
            Self::Advertisement(AdvertisementError::Revision) => "incompatible_revision",
            Self::X11(_) => "unavailable",
            Self::Advertisement(_) | Self::AdvertisementShape(_) | Self::AdvertisementUtf8 => {
                "malformed"
            }
        }
    }

    pub(crate) const fn retry(&self) -> &'static str {
        match self {
            Self::InvalidPath(_) | Self::Advertisement(AdvertisementError::Revision) => "never",
            _ => "reconnect",
        }
    }
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(source) => {
                write!(formatter, "{source} is not a bounded absolute socket path")
            }
            Self::X11(error) => write!(
                formatter,
                "cannot inspect X11 Agent Seat discovery: {error}"
            ),
            Self::Advertisement(error) => {
                write!(formatter, "invalid Agent Seat advertisement: {error}")
            }
            Self::AdvertisementShape(error) => {
                write!(formatter, "invalid Agent Seat advertisement: {error}")
            }
            Self::AdvertisementUtf8 => {
                formatter.write_str("invalid Agent Seat advertisement: property is not UTF-8")
            }
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// Resolves exactly one source: explicit, environment, then live X11.
pub(crate) fn resolve(explicit: Option<&Path>) -> Result<Option<PathBuf>, DiscoveryError> {
    if let Some(path) = explicit {
        return validate_path(path, "--socket").map(Some);
    }
    if let Some(path) = std::env::var_os("AGENT_SEAT_SOCKET") {
        return validate_path(Path::new(&path), "AGENT_SEAT_SOCKET").map(Some);
    }
    discover_x11()
}

fn validate_path(path: &Path, source: &'static str) -> Result<PathBuf, DiscoveryError> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.is_empty()
        || !path.is_absolute()
        || bytes.contains(&0)
        || bytes.len() > MAX_SOCKET_PATH_BYTES
    {
        return Err(DiscoveryError::InvalidPath(source));
    }
    Ok(path.to_path_buf())
}

fn discover_x11() -> Result<Option<PathBuf>, DiscoveryError> {
    if std::env::var_os("DISPLAY").is_none_or(|display| display.is_empty()) {
        return Ok(None);
    }
    let (connection, screen_index) = x11rb::connect(None).map_err(x11_error)?;
    let screen = connection
        .setup()
        .roots
        .get(screen_index)
        .ok_or_else(|| DiscoveryError::X11("selected screen is absent".to_owned()))?;
    let selection_name = format!("_AGENT_SEAT_S{screen_index}");
    let selection = atom(&connection, selection_name.as_bytes())?;
    if selection == NONE {
        return Ok(None);
    }
    let owner = connection
        .get_selection_owner(selection)
        .map_err(x11_error)?
        .reply()
        .map_err(x11_error)?
        .owner;
    if owner == NONE {
        return Ok(None);
    }
    let property = atom(&connection, ADVERTISEMENT_PROPERTY.as_bytes())?;
    let utf8 = atom(&connection, b"UTF8_STRING")?;
    if property == NONE || utf8 == NONE {
        return Ok(None);
    }
    let Some(owner_value) = read_property(&connection, owner, property, utf8)? else {
        return Ok(None);
    };
    let Some(root_value) = read_property(&connection, screen.root, property, utf8)? else {
        return Ok(None);
    };
    if owner_value != root_value {
        return Ok(None);
    }
    let current_owner = connection
        .get_selection_owner(selection)
        .map_err(x11_error)?
        .reply()
        .map_err(x11_error)?
        .owner;
    if current_owner != owner {
        return Ok(None);
    }
    let encoded =
        std::str::from_utf8(&root_value).map_err(|_| DiscoveryError::AdvertisementUtf8)?;
    let advertisement = Advertisement::parse(encoded).map_err(DiscoveryError::Advertisement)?;
    validate_path(Path::new(advertisement.socket()), "_AGENT_SEAT").map(Some)
}

fn atom<C: Connection>(connection: &C, name: &[u8]) -> Result<u32, DiscoveryError> {
    connection
        .intern_atom(true, name)
        .map_err(x11_error)?
        .reply()
        .map(|reply| reply.atom)
        .map_err(x11_error)
}

fn read_property<C: Connection>(
    connection: &C,
    window: u32,
    property: u32,
    utf8: u32,
) -> Result<Option<Vec<u8>>, DiscoveryError> {
    let reply = connection
        .get_property(
            false,
            window,
            property,
            AtomEnum::ANY,
            0,
            MAX_PROPERTY_LONGS,
        )
        .map_err(x11_error)?
        .reply()
        .map_err(x11_error)?;
    if reply.type_ == NONE {
        return Ok(None);
    }
    if reply.type_ != utf8
        || reply.format != 8
        || reply.bytes_after != 0
        || reply.value.len() > MAX_ADVERTISEMENT_BYTES
    {
        return Err(DiscoveryError::AdvertisementShape(
            "property type, format, or size is invalid",
        ));
    }
    Ok(Some(reply.value))
}

fn x11_error(error: impl fmt::Display) -> DiscoveryError {
    DiscoveryError::X11(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_paths_are_absolute_and_bounded() {
        assert!(validate_path(Path::new(""), "test").is_err());
        assert!(validate_path(Path::new("relative"), "test").is_err());
        assert!(validate_path(Path::new("/tmp/seat.sock"), "test").is_ok());
        let long = format!("/{}", "x".repeat(MAX_SOCKET_PATH_BYTES));
        assert!(validate_path(Path::new(&long), "test").is_err());
    }
}
