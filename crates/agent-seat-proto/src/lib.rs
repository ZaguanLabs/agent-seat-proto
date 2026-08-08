//! Display-server-neutral Agent Seat Protocol revision 3.
//!
//! This crate owns bounded wire values, strict JSON serialization, frame
//! encoding, and advertisement parsing. It owns no transport listener,
//! display-server integration, policy, application discovery, or MCP logic.

#![forbid(unsafe_code)]

mod advertisement;
mod bounded;
mod codec;
mod ids;
mod message;

pub use advertisement::{Advertisement, AdvertisementError};
pub use bounded::{BoundError, BoundedList, BoundedText};
pub use codec::{CodecError, ReadFrame, read_frame, write_frame};
pub use ids::{ClientId, Generation, LaunchToken, RequestId, Sequence, SessionId, WorkspaceId};
pub use message::*;

/// Protocol name carried by advertisements and opening messages.
pub const PROTOCOL_NAME: &str = "agent-seat";

/// Independently specified Tier 0 wire revision.
pub const PROTOCOL_REVISION: u16 = 3;

/// X11 property carrying the provider advertisement.
pub const ADVERTISEMENT_PROPERTY: &str = "_AGENT_SEAT";

/// Maximum encoded advertisement size in bytes.
pub const MAX_ADVERTISEMENT_BYTES: usize = 256;

/// Maximum client-to-provider frame size in bytes.
pub const MAX_REQUEST_FRAME_BYTES: usize = 64 * 1024;

/// Maximum provider-to-client frame size in bytes.
pub const MAX_RESPONSE_FRAME_BYTES: usize = 1024 * 1024;

/// Validation performed after strict deserialization and before use.
pub trait Validate {
    /// Returns a stable validation failure description.
    ///
    /// # Errors
    ///
    /// Returns an error when a cross-field invariant or uniqueness bound is
    /// violated.
    fn validate(&self) -> Result<(), &'static str>;
}
