//! Compact typed identities.

use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

macro_rules! nonzero_id {
    ($name:ident, $documentation:literal) => {
        #[doc = $documentation]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(NonZeroU64);

        impl $name {
            /// Creates a nonzero identity.
            #[must_use]
            pub const fn new(value: NonZeroU64) -> Self {
                Self(value)
            }

            /// Returns the numeric wire value.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}

nonzero_id!(RequestId, "A peer-selected request identity.");
nonzero_id!(SessionId, "A provider-selected session identity.");
nonzero_id!(ClientId, "A provider-session opaque client handle.");
nonzero_id!(LaunchToken, "A provider-selected launch identity.");

/// A monotonic provider-session observation cursor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Sequence(u64);

impl Sequence {
    /// Creates a sequence value. Zero names the state before the first
    /// observation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric wire value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Provider-local freshness for one client descriptor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Generation(u64);

impl Generation {
    /// Creates a generation value.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric wire value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A zero-based EWMH workspace index.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WorkspaceId(u16);

impl WorkspaceId {
    /// Creates an index.
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the numeric wire value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}
