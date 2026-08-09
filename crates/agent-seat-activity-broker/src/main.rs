//! Runtime broker: inherited read-only evdev descriptors in, coarse status out.

use std::env;
use std::fs::File;
use std::io::{ErrorKind, Read as _, Write as _};
use std::os::fd::{AsFd as _, OwnedFd};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use agent_seat_activity_broker::{
    BrokerState, BrokerStatus, CapabilityKind, ELIGIBILITY_FRAME_BYTES, EligibilityState,
    MAX_DEVICE_SET_BYTES, MAX_EVENT_DESCRIPTORS, StopReason, decode_device_set, read_eligibility,
    read_status_request, receive_inherited_files,
};
use evdev::raw_stream::RawDevice;
use evdev::{EventSummary, InputEvent, SynchronizationCode};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use rustix::net::sockopt::socket_peercred;
use rustix::process::geteuid;

const ARM_QUIET_INTERVAL: Duration = Duration::from_millis(250);
const INITIAL_ELIGIBILITY_WAIT: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agent-seat-activity-broker: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let arguments = Arguments::parse(env::args_os().skip(1))?;
    let broker_uid = geteuid().as_raw();
    if broker_uid == 0 {
        return Err("broker must run as a confined non-root service user".to_owned());
    }
    if broker_uid == arguments.uid {
        return Err("broker and enrolled provider must use distinct UIDs".to_owned());
    }
    let listener = open_inherited_listener()?;
    let mut inherited = receive_inherited_files(MAX_EVENT_DESCRIPTORS + 1, 1)
        .map_err(|error| format!("cannot receive inherited descriptors: {error}"))?;
    if inherited.first().map(|file| file.name()) != Some("enrolled-device-set") {
        return Err("first inherited descriptor must be named enrolled-device-set".to_owned());
    }
    let enrolled_devices = read_enrolled_device_set(inherited.remove(0).into_descriptor(), 0)?;
    if enrolled_devices.len() != inherited.len() {
        return Err("inherited event descriptor count does not match enrollment".to_owned());
    }
    let mut devices = inherited
        .into_iter()
        .enumerate()
        .map(|(index, file)| {
            if file.name() != format!("event{index}") {
                return Err(format!(
                    "inherited event descriptor {index} has the wrong name"
                ));
            }
            open_inherited_device(file.into_descriptor(), index)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if devices.is_empty() {
        return Err("at least one inherited event descriptor is required".to_owned());
    }
    for (device, enrolled) in devices.iter().zip(&enrolled_devices) {
        if !matches_device_capabilities(device, enrolled.capabilities()) {
            return Err(format!(
                "inherited event descriptor for event{} does not match enrollment",
                enrolled.event_number()
            ));
        }
    }
    let mut eligibility = open_inherited_eligibility()?;
    let initial_eligibility = read_initial_eligibility(&mut eligibility, INITIAL_ELIGIBILITY_WAIT)?;
    let initial_eligibility = matches!(initial_eligibility, EligibilityState::Eligible);
    let initial_device_update = initial_device_update(&mut devices)?;
    let instance = random_instance()?;
    let (state, reason) = if !initial_eligibility {
        (BrokerState::Stopped, StopReason::EligibilityChanged)
    } else if let Some(update) = initial_device_update {
        update
    } else {
        (BrokerState::Arming, StopReason::None)
    };
    let mut status = BrokerStatus {
        instance,
        epoch: 1,
        state,
        reason,
    };
    let usable_evidence = matches!(state, BrokerState::Arming | BrokerState::Ready);
    let mut eligibility = usable_evidence.then_some(eligibility);
    if !usable_evidence {
        devices.clear();
    }
    let mut arm_deadline =
        matches!(state, BrokerState::Arming).then(|| Instant::now() + ARM_QUIET_INTERVAL);
    let mut provider: Option<UnixStream> = None;

    loop {
        let listener_events;
        let provider_events;
        let eligibility_events;
        let device_events;
        {
            let mut polls = Vec::with_capacity(devices.len() + 3);
            let listener_index = if provider.is_none() {
                let index = polls.len();
                polls.push(PollFd::new(&listener, PollFlags::IN));
                Some(index)
            } else {
                None
            };
            let provider_index = provider.as_ref().map(|stream| {
                let index = polls.len();
                polls.push(PollFd::new(stream, PollFlags::IN));
                index
            });
            let eligibility_index = eligibility.as_ref().map(|source| {
                let index = polls.len();
                polls.push(PollFd::new(source, PollFlags::IN));
                index
            });
            let device_start = polls.len();
            polls.extend(
                devices
                    .iter()
                    .map(|device| PollFd::new(device, PollFlags::IN)),
            );
            let timeout = arm_deadline.map(|deadline| {
                let remaining = deadline.saturating_duration_since(Instant::now());
                Timespec {
                    tv_sec: i64::try_from(remaining.as_secs()).unwrap_or(i64::MAX),
                    tv_nsec: i64::from(remaining.subsec_nanos()),
                }
            });
            poll(&mut polls, timeout.as_ref()).map_err(|error| format!("poll failed: {error}"))?;
            listener_events = listener_index.map(|index| polls[index].revents());
            provider_events = provider_index.map(|index| polls[index].revents());
            eligibility_events = eligibility_index.map(|index| polls[index].revents());
            device_events = polls[device_start..]
                .iter()
                .map(PollFd::revents)
                .collect::<Vec<_>>();
        }

        let mut terminal_update = None;
        if let Some(events) = eligibility_events {
            if events.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
                terminal_update = Some((BrokerState::Unavailable, StopReason::EvidenceLost));
            } else if events.contains(PollFlags::IN) {
                let transition = match eligibility.as_mut().map(read_eligibility) {
                    Some(result) => result,
                    None => Err(agent_seat_activity_broker::ProtocolError::Unavailable),
                };
                terminal_update = Some(classify_eligibility_transition(transition));
            }
        }
        for (device, events) in devices.iter_mut().zip(device_events) {
            if terminal_update.is_some() {
                break;
            }
            if events.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
                terminal_update = Some((BrokerState::Unavailable, StopReason::EvidenceLost));
                break;
            }
            if !events.contains(PollFlags::IN) {
                continue;
            }
            let fetched = match device.fetch_events() {
                Ok(fetched) => fetched,
                Err(_) => {
                    terminal_update = Some((BrokerState::Unavailable, StopReason::EvidenceLost));
                    break;
                }
            };
            for event in fetched {
                if let Some(update) = classify_input_event(event) {
                    terminal_update = Some(update);
                    break;
                }
            }
            if terminal_update.is_some() {
                break;
            }
        }
        if let Some((state, reason)) = terminal_update {
            status = terminal(status, state, reason);
            devices.clear();
            eligibility = None;
            arm_deadline = None;
        } else if arm_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            status.state = BrokerState::Ready;
            arm_deadline = None;
        }

        if let Some(events) = listener_events {
            if events.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
                return Err("provider listener was lost".to_owned());
            }
            if events.contains(PollFlags::IN) {
                provider = accept_provider(&listener, arguments.uid)?;
            }
        }

        if let Some(events) = provider_events {
            if events.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
                provider = None;
            } else if events.contains(PollFlags::IN) {
                let keep = provider.as_mut().is_some_and(|stream| {
                    read_status_request(stream).is_ok()
                        && stream.write_all(&status.encode()).is_ok()
                        && stream.flush().is_ok()
                });
                if !keep {
                    provider = None;
                }
            }
        }
    }
}

fn open_inherited_listener() -> Result<UnixListener, String> {
    let descriptor = rustix::io::dup(std::io::stdout().as_fd())
        .map_err(|error| format!("standard output is not an inherited listener: {error}"))?;
    let listener = UnixListener::from(descriptor);
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("cannot make provider listener nonblocking: {error}"))?;
    listener
        .local_addr()
        .map_err(|error| format!("standard output is not a local socket listener: {error}"))?;
    Ok(listener)
}

fn open_inherited_eligibility() -> Result<File, String> {
    let descriptor = rustix::io::dup(std::io::stdin().as_fd())
        .map_err(|error| format!("standard input is not inherited eligibility: {error}"))?;
    let file = File::from(descriptor);
    let flags = fcntl_getfl(&file)
        .map_err(|error| format!("cannot inspect inherited eligibility flags: {error}"))?;
    fcntl_setfl(&file, flags | OFlags::NONBLOCK)
        .map_err(|error| format!("cannot make inherited eligibility nonblocking: {error}"))?;
    Ok(file)
}

fn read_initial_eligibility(
    source: &mut (impl std::io::Read + std::os::fd::AsFd),
    timeout: Duration,
) -> Result<EligibilityState, String> {
    let deadline = Instant::now() + timeout;
    let mut frame = [0_u8; ELIGIBILITY_FRAME_BYTES];
    let mut offset = 0;

    while offset < frame.len() {
        match source.read(&mut frame[offset..]) {
            Ok(0) => return Err("eligibility channel closed during initial frame".to_owned()),
            Ok(read) => {
                offset += read;
                continue;
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) => return Err(format!("cannot read initial eligibility evidence: {error}")),
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for initial eligibility evidence".to_owned());
        }
        let wait = Timespec {
            tv_sec: i64::try_from(remaining.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(remaining.subsec_nanos()),
        };
        let events = {
            let mut polls = [PollFd::new(&*source, PollFlags::IN)];
            poll(&mut polls, Some(&wait))
                .map_err(|error| format!("initial eligibility poll failed: {error}"))?;
            polls[0].revents()
        };
        if events.intersects(PollFlags::ERR | PollFlags::NVAL) {
            return Err("initial eligibility evidence was lost".to_owned());
        }
        if events.is_empty() {
            return Err("timed out waiting for initial eligibility evidence".to_owned());
        }
        if events.contains(PollFlags::HUP) && !events.contains(PollFlags::IN) {
            return Err("eligibility channel closed during initial frame".to_owned());
        }
    }

    read_eligibility(&mut frame.as_slice())
        .map_err(|error| format!("cannot decode initial eligibility evidence: {error:?}"))
}

fn accept_provider(listener: &UnixListener, uid: u32) -> Result<Option<UnixStream>, String> {
    let (stream, _) = match listener.accept() {
        Ok(accepted) => accepted,
        Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(None),
        Err(error) => return Err(format!("cannot accept provider connection: {error}")),
    };
    let peer = socket_peercred(&stream)
        .map_err(|error| format!("cannot authenticate provider connection: {error}"))?;
    if peer.uid.as_raw() != uid {
        return Ok(None);
    }
    stream
        .set_nonblocking(true)
        .map_err(|error| format!("cannot make provider connection nonblocking: {error}"))?;
    Ok(Some(stream))
}

fn terminal(mut status: BrokerStatus, state: BrokerState, reason: StopReason) -> BrokerStatus {
    status.epoch = status.epoch.saturating_add(1);
    status.state = state;
    status.reason = reason;
    status
}

fn classify_eligibility_transition(
    transition: Result<EligibilityState, agent_seat_activity_broker::ProtocolError>,
) -> (BrokerState, StopReason) {
    match transition {
        Ok(EligibilityState::Ineligible) => (BrokerState::Stopped, StopReason::EligibilityChanged),
        Ok(EligibilityState::Eligible) | Err(_) => {
            (BrokerState::Unavailable, StopReason::EvidenceLost)
        }
    }
}

fn classify_input_event(event: InputEvent) -> Option<(BrokerState, StopReason)> {
    match event.destructure() {
        EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => None,
        EventSummary::Synchronization(_, SynchronizationCode::SYN_DROPPED, _) => {
            Some((BrokerState::Unavailable, StopReason::EventLoss))
        }
        _ => Some((BrokerState::Stopped, StopReason::Activity)),
    }
}

fn initial_device_update(
    devices: &mut [RawDevice],
) -> Result<Option<(BrokerState, StopReason)>, String> {
    for device in devices.iter() {
        if device.supported_keys().is_some() {
            let state = device
                .get_key_state()
                .map_err(|error| format!("cannot inspect initial key/button state: {error}"))?;
            if (&state).into_iter().next().is_some() {
                return Ok(Some((BrokerState::Stopped, StopReason::Activity)));
            }
        }
    }

    let mut polls = devices
        .iter()
        .map(|device| PollFd::new(device, PollFlags::IN))
        .collect::<Vec<_>>();
    let no_wait = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    poll(&mut polls, Some(&no_wait))
        .map_err(|error| format!("initial device poll failed: {error}"))?;
    let events = polls.iter().map(PollFd::revents).collect::<Vec<_>>();
    let mut activity = false;
    for (device, events) in devices.iter_mut().zip(events) {
        if events.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
            return Ok(Some((BrokerState::Unavailable, StopReason::EvidenceLost)));
        }
        if !events.contains(PollFlags::IN) {
            continue;
        }
        let fetched = device
            .fetch_events()
            .map_err(|error| format!("cannot inspect initial event queue: {error}"))?;
        for event in fetched {
            match classify_input_event(event) {
                Some((BrokerState::Unavailable, reason)) => {
                    return Ok(Some((BrokerState::Unavailable, reason)));
                }
                Some((BrokerState::Stopped, StopReason::Activity)) => activity = true,
                Some(_) | None => {}
            }
        }
    }
    Ok(activity.then_some((BrokerState::Stopped, StopReason::Activity)))
}

fn open_inherited_device(descriptor: OwnedFd, index: usize) -> Result<RawDevice, String> {
    let descriptor = confine_inherited_file(descriptor, "event")?;
    RawDevice::from_fd(descriptor)
        .map_err(|error| format!("event descriptor {index} is not an evdev device: {error}"))
}

fn read_enrolled_device_set(
    descriptor: OwnedFd,
    owner_uid: u32,
) -> Result<Vec<agent_seat_activity_broker::EnrolledDevice>, String> {
    let file = File::from(confine_inherited_file(descriptor, "device-set")?);
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect enrolled device set: {error}"))?;
    if !metadata.file_type().is_file()
        || metadata.nlink() != 1
        || metadata.uid() != owner_uid
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() > u64::try_from(MAX_DEVICE_SET_BYTES).unwrap_or(u64::MAX)
    {
        return Err("enrolled device set is not an exact private peer-owned file".to_owned());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(u64::try_from(MAX_DEVICE_SET_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read enrolled device set: {error}"))?;
    if bytes.len() > MAX_DEVICE_SET_BYTES {
        return Err("enrolled device set exceeds its read bound".to_owned());
    }
    decode_device_set(&bytes).map_err(|error| format!("invalid enrolled device set: {error}"))
}

fn matches_device_capabilities(
    device: &RawDevice,
    expected: &agent_seat_activity_broker::DeviceCapabilities,
) -> bool {
    expected.matches(
        CapabilityKind::Event,
        device.supported_events().iter().map(|code| code.0),
    ) && expected.matches(
        CapabilityKind::Key,
        device
            .supported_keys()
            .into_iter()
            .flat_map(|codes| codes.iter())
            .map(|code| code.0),
    ) && expected.matches(
        CapabilityKind::Relative,
        device
            .supported_relative_axes()
            .into_iter()
            .flat_map(|codes| codes.iter())
            .map(|code| code.0),
    ) && expected.matches(
        CapabilityKind::Absolute,
        device
            .supported_absolute_axes()
            .into_iter()
            .flat_map(|codes| codes.iter())
            .map(|code| code.0),
    ) && expected.matches(
        CapabilityKind::Misc,
        device
            .misc_properties()
            .into_iter()
            .flat_map(|codes| codes.iter())
            .map(|code| code.0),
    ) && expected.matches(
        CapabilityKind::Switch,
        device
            .supported_switches()
            .into_iter()
            .flat_map(|codes| codes.iter())
            .map(|code| code.0),
    ) && expected.matches(
        CapabilityKind::Led,
        device
            .supported_leds()
            .into_iter()
            .flat_map(|codes| codes.iter())
            .map(|code| code.0),
    ) && expected.matches(
        CapabilityKind::Sound,
        device
            .supported_sounds()
            .into_iter()
            .flat_map(|codes| codes.iter())
            .map(|code| code.0),
    ) && expected.matches(
        CapabilityKind::ForceFeedback,
        device
            .supported_ff()
            .into_iter()
            .flat_map(|codes| codes.iter())
            .map(|code| code.0),
    ) && expected.matches(
        CapabilityKind::Property,
        device.properties().iter().map(|code| code.0),
    )
}

fn confine_inherited_file(descriptor: OwnedFd, kind: &str) -> Result<OwnedFd, String> {
    let flags = fcntl_getfl(&descriptor)
        .map_err(|error| format!("cannot inspect {kind} descriptor flags: {error}"))?;
    fcntl_setfl(&descriptor, flags | OFlags::NONBLOCK)
        .map_err(|error| format!("cannot make {kind} descriptor nonblocking: {error}"))?;
    Ok(descriptor)
}

fn random_instance() -> Result<u64, String> {
    let mut bytes = [0_u8; 8];
    let mut offset = 0;
    while offset < bytes.len() {
        let count =
            rustix::rand::getrandom(&mut bytes[offset..], rustix::rand::GetRandomFlags::empty())
                .map_err(|error| format!("cannot create broker instance identifier: {error}"))?;
        if count == 0 {
            return Err("random source returned no instance bytes".to_owned());
        }
        offset += count;
    }
    let instance = u64::from_ne_bytes(bytes);
    if instance == 0 {
        return Err("random broker instance identifier was zero".to_owned());
    }
    Ok(instance)
}

struct Arguments {
    uid: u32,
}

impl Arguments {
    fn parse(arguments: impl Iterator<Item = std::ffi::OsString>) -> Result<Self, String> {
        let mut arguments = arguments;
        let mut uid = None;
        while let Some(argument) = arguments.next() {
            let argument = argument
                .to_str()
                .ok_or_else(|| "arguments must be UTF-8".to_owned())?;
            match argument {
                "--uid" => {
                    if uid.is_some() {
                        return Err("--uid may be specified once".to_owned());
                    }
                    uid = Some(parse_u32(arguments.next(), "--uid")?);
                }
                "--help" => {
                    println!(
                        "Usage: agent-seat-activity-broker --uid UID\n\
                         Adopts the exact named descriptors addressed by systemd. \
                         Reads eligibility from preconnected standard input and provider requests \
                         from an authenticated AF_UNIX listener on standard output."
                    );
                    std::process::exit(0);
                }
                _ => return Err(format!("unknown argument {argument:?}")),
            }
        }
        Ok(Self {
            uid: uid.ok_or_else(|| "--uid is required".to_owned())?,
        })
    }
}

fn parse_u32(value: Option<std::ffi::OsString>, option: &str) -> Result<u32, String> {
    value
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| format!("{option} requires a value"))?
        .parse()
        .map_err(|_| format!("{option} requires an unsigned integer"))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;

    use agent_seat_activity_broker::{ProtocolError, write_eligibility};
    use evdev::{EventType, KeyCode};

    use super::*;

    fn arguments(values: &[&str]) -> Result<Arguments, String> {
        Arguments::parse(values.iter().map(OsString::from))
    }

    #[test]
    fn arguments_accept_only_one_enrolled_uid() {
        let parsed = arguments(&["--uid", "1000"]).expect("valid broker arguments");
        assert_eq!(parsed.uid, 1000);
        assert!(arguments(&["--uid", "1000", "--event-fd", "3"]).is_err());
    }

    #[test]
    fn initial_eligibility_wait_accepts_one_complete_frame() {
        let (mut source, mut sink) = UnixStream::pair().expect("eligibility socket pair");
        source
            .set_nonblocking(true)
            .expect("nonblocking eligibility source");
        write_eligibility(&mut sink, EligibilityState::Eligible).expect("eligibility frame");

        assert_eq!(
            read_initial_eligibility(&mut source, Duration::ZERO),
            Ok(EligibilityState::Eligible)
        );
    }

    #[test]
    fn initial_eligibility_wait_rejects_a_partial_frame_without_blocking() {
        let (mut source, mut sink) = UnixStream::pair().expect("eligibility socket pair");
        source
            .set_nonblocking(true)
            .expect("nonblocking eligibility source");
        sink.write_all(b"ASEL").expect("partial eligibility frame");

        let error = read_initial_eligibility(&mut source, Duration::ZERO)
            .expect_err("partial frame must fail closed");
        assert!(error.contains("timed out"));
    }

    #[test]
    fn eligibility_transition_never_reasserts_ready() {
        assert_eq!(
            classify_eligibility_transition(Ok(EligibilityState::Ineligible)),
            (BrokerState::Stopped, StopReason::EligibilityChanged)
        );
        assert_eq!(
            classify_eligibility_transition(Ok(EligibilityState::Eligible)),
            (BrokerState::Unavailable, StopReason::EvidenceLost)
        );
        assert_eq!(
            classify_eligibility_transition(Err(ProtocolError::Unavailable)),
            (BrokerState::Unavailable, StopReason::EvidenceLost)
        );
    }

    #[test]
    fn terminal_transition_advances_epoch_and_preserves_instance() {
        let initial = BrokerStatus {
            instance: 19,
            epoch: 1,
            state: BrokerState::Ready,
            reason: StopReason::None,
        };
        let stopped = terminal(
            initial,
            BrokerState::Stopped,
            StopReason::EligibilityChanged,
        );
        assert_eq!(stopped.instance, initial.instance);
        assert_eq!(stopped.epoch, 2);
        assert_eq!(stopped.state, BrokerState::Stopped);
        assert_eq!(stopped.reason, StopReason::EligibilityChanged);
    }

    #[test]
    fn evdev_report_activity_and_loss_have_distinct_terminal_effects() {
        assert_eq!(
            classify_input_event(InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            )),
            None
        );
        assert_eq!(
            classify_input_event(InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_DROPPED.0,
                0,
            )),
            Some((BrokerState::Unavailable, StopReason::EventLoss))
        );
        assert_eq!(
            classify_input_event(InputEvent::new(EventType::KEY.0, KeyCode::KEY_A.0, 1)),
            Some((BrokerState::Stopped, StopReason::Activity))
        );
    }
}
