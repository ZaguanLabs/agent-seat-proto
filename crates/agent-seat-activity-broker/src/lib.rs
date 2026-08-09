//! Minimal fail-closed status protocol and bounded deployment metadata shared
//! by the activity components. Raw event contents are not represented here.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use rustix::net::sockopt::socket_peercred;

mod device_set;
#[cfg(any(feature = "eligibility-guard", feature = "runtime"))]
mod inherited_files;
mod initial_input_set;

pub use device_set::{
    CapabilityKind, DeviceCapabilities, DeviceIdentity, DeviceSetError, EnrolledDevice,
    IdentityStrength, MAX_DEVICE_SET_BYTES, decode_device_set, encode_device_set,
};
#[cfg(any(feature = "eligibility-guard", feature = "runtime"))]
pub use inherited_files::{InheritedFile, InheritedFileError, receive_inherited_files};
pub use initial_input_set::{
    InputClassMapping, InputSetError, MAX_INPUT_CLASS_MAPPINGS, MAX_INPUT_SET_BYTES,
    decode_input_set, encode_input_set,
};

const MAGIC: [u8; 4] = *b"ASAB";
const REVISION: u16 = 1;
const OP_STATUS: u8 = 1;
const REQUEST_BYTES: usize = 8;
const RESPONSE_BYTES: usize = 24;
const ELIGIBILITY_MAGIC: [u8; 4] = *b"ASEL";
const ELIGIBILITY_REVISION: u16 = 1;
/// Fixed byte length of one eligibility frame.
pub const ELIGIBILITY_FRAME_BYTES: usize = 8;

/// Maximum exact evdev descriptor set accepted by inspection and runtime.
pub const MAX_EVENT_DESCRIPTORS: usize = 32;

/// One minimal assertion from the independently trusted session/lock source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EligibilityState {
    /// The enrolled session is currently eligible to arm.
    Eligible,
    /// A safety transition occurred and this broker instance cannot rearm.
    Ineligible,
}

impl EligibilityState {
    const fn encode(self) -> u8 {
        match self {
            Self::Eligible => 1,
            Self::Ineligible => 2,
        }
    }

    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Eligible),
            2 => Ok(Self::Ineligible),
            _ => Err(ProtocolError::Malformed),
        }
    }
}

/// Broker readiness visible to the X11 provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerState {
    /// Initial evidence is valid but the bounded quiet interval is incomplete.
    Arming,
    /// Exact device and session evidence is ready.
    Ready,
    /// Physical activity permanently stopped this broker instance.
    Stopped,
    /// Required evidence was lost or could not be interpreted.
    Unavailable,
}

impl BrokerState {
    const fn encode(self) -> u8 {
        match self {
            Self::Arming => 4,
            Self::Ready => 1,
            Self::Stopped => 2,
            Self::Unavailable => 3,
        }
    }

    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            4 => Ok(Self::Arming),
            1 => Ok(Self::Ready),
            2 => Ok(Self::Stopped),
            3 => Ok(Self::Unavailable),
            _ => Err(ProtocolError::Malformed),
        }
    }
}

/// Coarse terminal reason that reveals no event or device identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    /// No terminal reason while ready.
    None,
    /// Physical activity was observed.
    Activity,
    /// An event stream overflowed or became ambiguous.
    EventLoss,
    /// A required descriptor or session signal was lost.
    EvidenceLost,
    /// An internal invariant failed.
    Internal,
    /// Trusted session or lock eligibility changed.
    EligibilityChanged,
}

impl StopReason {
    const fn encode(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Activity => 1,
            Self::EventLoss => 2,
            Self::EvidenceLost => 3,
            Self::Internal => 4,
            Self::EligibilityChanged => 5,
        }
    }

    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Activity),
            2 => Ok(Self::EventLoss),
            3 => Ok(Self::EvidenceLost),
            4 => Ok(Self::Internal),
            5 => Ok(Self::EligibilityChanged),
            _ => Err(ProtocolError::Malformed),
        }
    }
}

/// One immutable broker-instance observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrokerStatus {
    /// Random identifier replaced on every broker start.
    pub instance: u64,
    /// Monotonic activity/safety epoch.
    pub epoch: u64,
    /// Current readiness state.
    pub state: BrokerState,
    /// Coarse terminal reason.
    pub reason: StopReason,
}

impl BrokerStatus {
    /// Returns whether two ready observations prove an unchanged gate.
    #[must_use]
    pub const fn is_same_ready(self, later: Self) -> bool {
        matches!(self.state, BrokerState::Ready)
            && matches!(later.state, BrokerState::Ready)
            && self.instance == later.instance
            && self.epoch == later.epoch
    }

    /// Encodes one fixed-size response without event data.
    #[must_use]
    pub fn encode(self) -> [u8; RESPONSE_BYTES] {
        let mut bytes = [0_u8; RESPONSE_BYTES];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4..6].copy_from_slice(&REVISION.to_be_bytes());
        bytes[6] = self.state.encode();
        bytes[7] = self.reason.encode();
        bytes[8..16].copy_from_slice(&self.instance.to_be_bytes());
        bytes[16..24].copy_from_slice(&self.epoch.to_be_bytes());
        bytes
    }

    fn decode(bytes: [u8; RESPONSE_BYTES]) -> Result<Self, ProtocolError> {
        if bytes[..4] != MAGIC || u16::from_be_bytes([bytes[4], bytes[5]]) != REVISION {
            return Err(ProtocolError::Incompatible);
        }
        let state = BrokerState::decode(bytes[6])?;
        let reason = StopReason::decode(bytes[7])?;
        let valid_state = matches!(
            (state, reason),
            (BrokerState::Arming | BrokerState::Ready, StopReason::None)
                | (BrokerState::Stopped, StopReason::Activity)
                | (BrokerState::Stopped, StopReason::EligibilityChanged)
                | (
                    BrokerState::Unavailable,
                    StopReason::EventLoss | StopReason::EvidenceLost | StopReason::Internal
                )
        );
        if !valid_state {
            return Err(ProtocolError::Malformed);
        }
        let instance = u64::from_be_bytes(
            bytes[8..16]
                .try_into()
                .map_err(|_| ProtocolError::Malformed)?,
        );
        let epoch = u64::from_be_bytes(
            bytes[16..24]
                .try_into()
                .map_err(|_| ProtocolError::Malformed)?,
        );
        if instance == 0 || epoch == 0 {
            return Err(ProtocolError::Malformed);
        }
        Ok(Self {
            instance,
            epoch,
            state,
            reason,
        })
    }
}

/// Fixed protocol or transport failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    /// The broker socket could not complete bounded I/O.
    Unavailable,
    /// The peer uses a different broker protocol revision.
    Incompatible,
    /// A fixed frame contains impossible fields.
    Malformed,
}

/// Queries one fresh broker status over a new bounded local connection.
///
/// # Errors
///
/// Returns [`ProtocolError`] on connection, timeout, framing, revision, or
/// state-invariant failure.
pub fn query(
    path: &Path,
    timeout: Duration,
    expected_uid: u32,
) -> Result<BrokerStatus, ProtocolError> {
    BrokerConnection::connect(path, timeout, expected_uid)?.status()
}

/// One bounded persistent provider connection to a broker instance.
pub struct BrokerConnection {
    stream: UnixStream,
}

impl BrokerConnection {
    /// Opens one local broker instance and applies read/write timeouts.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::Unavailable`] when the socket cannot be opened
    /// or bounded.
    pub fn connect(
        path: &Path,
        timeout: Duration,
        expected_uid: u32,
    ) -> Result<Self, ProtocolError> {
        let stream = UnixStream::connect(path).map_err(|_| ProtocolError::Unavailable)?;
        let peer = socket_peercred(&stream).map_err(|_| ProtocolError::Unavailable)?;
        if peer.uid.as_raw() != expected_uid {
            return Err(ProtocolError::Unavailable);
        }
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .map_err(|_| ProtocolError::Unavailable)?;
        Ok(Self { stream })
    }

    /// Reads one fresh status from the same broker instance.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] on transport, framing, or state failure.
    pub fn status(&mut self) -> Result<BrokerStatus, ProtocolError> {
        write_status_request(&mut self.stream)?;
        read_status_response(&mut self.stream)
    }
}

/// Writes the one allowed request to an already authenticated broker stream.
///
/// # Errors
///
/// Returns [`ProtocolError::Unavailable`] when the fixed frame cannot be
/// written completely.
pub fn write_status_request(stream: &mut impl Write) -> Result<(), ProtocolError> {
    let mut bytes = [0_u8; REQUEST_BYTES];
    bytes[..4].copy_from_slice(&MAGIC);
    bytes[4..6].copy_from_slice(&REVISION.to_be_bytes());
    bytes[6] = OP_STATUS;
    stream
        .write_all(&bytes)
        .map_err(|_| ProtocolError::Unavailable)
}

/// Reads and validates the one allowed request.
///
/// # Errors
///
/// Returns [`ProtocolError`] on truncated, incompatible, or malformed input.
pub fn read_status_request(stream: &mut impl Read) -> Result<(), ProtocolError> {
    let mut bytes = [0_u8; REQUEST_BYTES];
    stream
        .read_exact(&mut bytes)
        .map_err(|_| ProtocolError::Unavailable)?;
    if bytes[..4] != MAGIC || u16::from_be_bytes([bytes[4], bytes[5]]) != REVISION {
        return Err(ProtocolError::Incompatible);
    }
    if bytes[6] != OP_STATUS || bytes[7] != 0 {
        return Err(ProtocolError::Malformed);
    }
    Ok(())
}

/// Reads and validates one fixed-size status response.
///
/// # Errors
///
/// Returns [`ProtocolError`] on truncated or invalid input.
pub fn read_status_response(stream: &mut impl Read) -> Result<BrokerStatus, ProtocolError> {
    let mut bytes = [0_u8; RESPONSE_BYTES];
    stream
        .read_exact(&mut bytes)
        .map_err(|_| ProtocolError::Unavailable)?;
    BrokerStatus::decode(bytes)
}

/// Writes one fixed assertion to the broker's inherited eligibility channel.
///
/// # Errors
///
/// Returns [`ProtocolError::Unavailable`] when the frame cannot be written.
pub fn write_eligibility(
    stream: &mut impl Write,
    state: EligibilityState,
) -> Result<(), ProtocolError> {
    let mut frame = [0_u8; ELIGIBILITY_FRAME_BYTES];
    frame[..4].copy_from_slice(&ELIGIBILITY_MAGIC);
    frame[4..6].copy_from_slice(&ELIGIBILITY_REVISION.to_be_bytes());
    frame[6] = state.encode();
    stream
        .write_all(&frame)
        .map_err(|_| ProtocolError::Unavailable)
}

/// Reads one complete assertion from the inherited eligibility channel.
///
/// # Errors
///
/// Returns [`ProtocolError`] on loss, truncation, incompatible revision, or an
/// unknown state. The runtime must permanently fail closed after any error.
pub fn read_eligibility(stream: &mut impl Read) -> Result<EligibilityState, ProtocolError> {
    let mut frame = [0_u8; ELIGIBILITY_FRAME_BYTES];
    stream
        .read_exact(&mut frame)
        .map_err(|_| ProtocolError::Unavailable)?;
    if frame[..4] != ELIGIBILITY_MAGIC
        || u16::from_be_bytes([frame[4], frame[5]]) != ELIGIBILITY_REVISION
        || frame[7] != 0
    {
        return Err(ProtocolError::Incompatible);
    }
    EligibilityState::decode(frame[6])
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::thread;

    use rustix::process::geteuid;

    use super::*;

    #[test]
    fn fixed_status_round_trips_and_rejects_impossible_ready_reason() {
        for state in [BrokerState::Arming, BrokerState::Ready] {
            let status = BrokerStatus {
                instance: 9,
                epoch: 4,
                state,
                reason: StopReason::None,
            };
            assert_eq!(BrokerStatus::decode(status.encode()), Ok(status));
        }

        let mut invalid = BrokerStatus {
            instance: 9,
            epoch: 4,
            state: BrokerState::Ready,
            reason: StopReason::None,
        }
        .encode();
        invalid[7] = StopReason::Activity.encode();
        assert_eq!(BrokerStatus::decode(invalid), Err(ProtocolError::Malformed));
    }

    #[test]
    fn unchanged_gate_requires_same_ready_instance_and_epoch() {
        let ready = BrokerStatus {
            instance: 7,
            epoch: 11,
            state: BrokerState::Ready,
            reason: StopReason::None,
        };
        assert!(ready.is_same_ready(ready));
        assert!(!ready.is_same_ready(BrokerStatus { epoch: 12, ..ready }));
        assert!(!ready.is_same_ready(BrokerStatus {
            state: BrokerState::Stopped,
            reason: StopReason::Activity,
            ..ready
        }));
        assert!(!ready.is_same_ready(BrokerStatus {
            state: BrokerState::Arming,
            ..ready
        }));
    }

    #[test]
    fn provider_rejects_a_socket_owned_by_another_uid() {
        let path = std::env::temp_dir().join(format!(
            "agent-seat-broker-peer-{}-{}.sock",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind broker identity fixture");
        let server = thread::spawn(move || {
            let _ = listener.accept().expect("accept identity fixture");
        });
        let actual_uid = geteuid().as_raw();
        let wrong_uid = actual_uid.checked_add(1).unwrap_or(0);
        assert!(matches!(
            BrokerConnection::connect(&path, Duration::from_secs(1), wrong_uid),
            Err(ProtocolError::Unavailable)
        ));
        server.join().expect("identity fixture");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn eligibility_frames_are_fixed_and_strict() {
        for state in [EligibilityState::Eligible, EligibilityState::Ineligible] {
            let mut frame = Vec::new();
            write_eligibility(&mut frame, state).expect("encode eligibility fixture");
            assert_eq!(frame.len(), ELIGIBILITY_FRAME_BYTES);
            assert_eq!(read_eligibility(&mut frame.as_slice()), Ok(state));
        }

        let mut malformed = Vec::new();
        write_eligibility(&mut malformed, EligibilityState::Eligible)
            .expect("encode malformed fixture baseline");
        malformed[6] = u8::MAX;
        assert_eq!(
            read_eligibility(&mut malformed.as_slice()),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            read_eligibility(&mut &malformed[..malformed.len() - 1]),
            Err(ProtocolError::Unavailable)
        );
    }
}
