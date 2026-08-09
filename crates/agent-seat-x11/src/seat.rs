//! Volatile, provider-owned operator gate and its private local control plane.

use std::io::{Read as _, Write as _};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rustix::net::sockopt::socket_peercred;
use rustix::process::geteuid;

const CONTROL_MAGIC: &[u8; 4] = b"ASG1";
const REQUEST_BYTES: usize = CONTROL_MAGIC.len() + 1;
const RESPONSE_BYTES: usize = CONTROL_MAGIC.len() + 1 + size_of::<u64>();
const CONTROL_TIMEOUT: Duration = Duration::from_millis(250);
const STATUS: u8 = 0;
const ENABLE: u8 = 1;
const DISABLE: u8 = 2;
const DISABLED_TERMINAL: u64 = u64::MAX;

/// One provider-instance-local authorization generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SeatPermit(u64);

/// A volatile gate that starts disabled and revokes old sessions on transition.
pub(crate) struct SeatGate(AtomicU64);

impl SeatGate {
    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    pub(crate) fn permit(&self) -> Option<SeatPermit> {
        let state = self.0.load(Ordering::Acquire);
        is_enabled(state).then_some(SeatPermit(state))
    }

    pub(crate) fn accepts(&self, permit: SeatPermit) -> bool {
        self.0.load(Ordering::Acquire) == permit.0 && is_enabled(permit.0)
    }

    fn status(&self) -> SeatStatus {
        SeatStatus::from_raw(self.0.load(Ordering::Acquire))
    }

    fn set_enabled(&self, enabled: bool) -> Result<SeatStatus, String> {
        let mut current = self.0.load(Ordering::Acquire);
        loop {
            let status = SeatStatus::from_raw(current);
            if status.enabled == enabled {
                return Ok(status);
            }
            let Some(next) = current.checked_add(1) else {
                self.0.store(DISABLED_TERMINAL, Ordering::Release);
                return if enabled {
                    Err("seat transition space is exhausted; restart the provider".to_owned())
                } else {
                    Ok(SeatStatus::from_raw(DISABLED_TERMINAL))
                };
            };
            if next == DISABLED_TERMINAL {
                self.0.store(DISABLED_TERMINAL, Ordering::Release);
                return Err("seat transition space is exhausted; restart the provider".to_owned());
            }
            match self
                .0
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return Ok(SeatStatus::from_raw(next)),
                Err(observed) => current = observed,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SeatStatus {
    enabled: bool,
    generation: u64,
}

impl SeatStatus {
    const fn from_raw(raw: u64) -> Self {
        Self {
            enabled: is_enabled(raw),
            generation: raw / 2,
        }
    }
}

const fn is_enabled(raw: u64) -> bool {
    raw != DISABLED_TERMINAL && raw & 1 == 1
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControlCommand {
    Status,
    Enable,
    Disable,
}

impl ControlCommand {
    const fn code(self) -> u8 {
        match self {
            Self::Status => STATUS,
            Self::Enable => ENABLE,
            Self::Disable => DISABLE,
        }
    }
}

pub(crate) fn handle_pending(listener: &UnixListener, gate: &SeatGate) -> Result<(), String> {
    let (mut stream, _) = match listener.accept() {
        Ok(accepted) => accepted,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
        Err(error) => return Err(format!("seat-control accept failed: {error}")),
    };
    let peer = socket_peercred(&stream)
        .map_err(|error| format!("cannot authenticate seat-control peer: {error}"))?;
    if peer.uid != geteuid() {
        return Ok(());
    }
    stream
        .set_read_timeout(Some(CONTROL_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(CONTROL_TIMEOUT)))
        .map_err(|error| format!("cannot bound seat-control I/O: {error}"))?;

    let mut request = [0_u8; REQUEST_BYTES];
    if stream.read_exact(&mut request).is_err() || &request[..CONTROL_MAGIC.len()] != CONTROL_MAGIC
    {
        return Ok(());
    }
    let status = match request[CONTROL_MAGIC.len()] {
        STATUS => gate.status(),
        ENABLE => gate.set_enabled(true)?,
        DISABLE => gate.set_enabled(false)?,
        _ => return Ok(()),
    };
    stream
        .write_all(&encode_status(status))
        .map_err(|error| format!("cannot acknowledge seat-control request: {error}"))
}

pub(crate) fn request(path: &std::path::Path, command: ControlCommand) -> Result<String, String> {
    let mut stream = UnixStream::connect(path)
        .map_err(|error| format!("cannot connect to running provider seat control: {error}"))?;
    stream
        .set_read_timeout(Some(CONTROL_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(CONTROL_TIMEOUT)))
        .map_err(|error| format!("cannot bound seat-control I/O: {error}"))?;
    let mut request = [0_u8; REQUEST_BYTES];
    request[..CONTROL_MAGIC.len()].copy_from_slice(CONTROL_MAGIC);
    request[CONTROL_MAGIC.len()] = command.code();
    stream
        .write_all(&request)
        .and_then(|()| stream.shutdown(Shutdown::Write))
        .map_err(|error| format!("cannot send seat-control request: {error}"))?;
    let mut response = [0_u8; RESPONSE_BYTES];
    stream
        .read_exact(&mut response)
        .map_err(|error| format!("cannot read seat-control response: {error}"))?;
    let status = decode_status(response)?;
    let state = if status.enabled {
        "enabled"
    } else {
        "disabled"
    };
    Ok(format!("Seat {state} (generation {}).", status.generation))
}

fn encode_status(status: SeatStatus) -> [u8; RESPONSE_BYTES] {
    let mut response = [0_u8; RESPONSE_BYTES];
    response[..CONTROL_MAGIC.len()].copy_from_slice(CONTROL_MAGIC);
    response[CONTROL_MAGIC.len()] = u8::from(status.enabled);
    response[CONTROL_MAGIC.len() + 1..].copy_from_slice(&status.generation.to_le_bytes());
    response
}

fn decode_status(response: [u8; RESPONSE_BYTES]) -> Result<SeatStatus, String> {
    if &response[..CONTROL_MAGIC.len()] != CONTROL_MAGIC {
        return Err("provider returned an invalid seat-control response".to_owned());
    }
    let enabled = match response[CONTROL_MAGIC.len()] {
        0 => false,
        1 => true,
        _ => return Err("provider returned an invalid seat-control state".to_owned()),
    };
    let mut generation = [0_u8; size_of::<u64>()];
    generation.copy_from_slice(&response[CONTROL_MAGIC.len() + 1..]);
    Ok(SeatStatus {
        enabled,
        generation: u64::from_le_bytes(generation),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_starts_disabled_and_each_transition_revokes_old_permits() {
        let gate = SeatGate::new();
        assert_eq!(gate.permit(), None);
        assert_eq!(gate.set_enabled(true).expect("enable").generation, 0);
        let first = gate.permit().expect("first permit");
        assert!(gate.accepts(first));
        assert!(!gate.set_enabled(false).expect("disable").enabled);
        assert!(!gate.accepts(first));
        assert_eq!(gate.set_enabled(true).expect("re-enable").generation, 1);
        let second = gate.permit().expect("second permit");
        assert_ne!(first, second);
        assert!(gate.accepts(second));
    }

    #[test]
    fn control_status_has_a_strict_fixed_encoding() {
        let status = SeatStatus {
            enabled: true,
            generation: 37,
        };
        assert_eq!(decode_status(encode_status(status)), Ok(status));
        let mut invalid = encode_status(status);
        invalid[0] = b'X';
        assert!(decode_status(invalid).is_err());
    }
}
