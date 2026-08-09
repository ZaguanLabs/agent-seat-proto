//! Fail-closed logind eligibility reduction for the experimental broker.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read as _;
use std::os::fd::{AsFd as _, OwnedFd};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use agent_seat_activity_broker::{
    EligibilityState, InputClassMapping, MAX_INPUT_CLASS_MAPPINGS, MAX_INPUT_SET_BYTES,
    decode_input_set, receive_inherited_files, write_eligibility,
};
use dbus::Path as DbusPath;
use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::{Properties, PropertiesPropertiesChanged};
use dbus::message::MatchRule;
use rustix::net::sockopt::socket_peercred;
use rustix::net::{
    AddressFamily, RecvFlags, SocketFlags, SocketType, bind, netlink, recvfrom, socket_with,
};

const HELP: &str = r#"agent-seat-eligibility-guard - reduce logind state to one fail-closed channel

USAGE:
  agent-seat-eligibility-guard --session ID --uid UID --seat seat0 \
      (--socket PATH | --listen-stdin) [--peer-uid UID]
  agent-seat-eligibility-guard --help

OPTIONS:
  --session ID   Exact enrolled logind session ID
  --uid UID      Exact enrolled session-owner UID
  --seat seat0   Exact enrolled physical seat
  --socket PATH  New absolute private AF_UNIX socket for the broker connection
  --listen-stdin Use one service-manager-owned AF_UNIX listener on stdin
  --peer-uid UID Expected connecting service-manager UID; defaults to 0
  -h, --help     Print this help

The guard has no input-event, X11, MCP, launch, policy, or injection access.
It receives only kernel device-lifecycle notifications through a bounded
AF_NETLINK socket. It sends one initial eligibility frame and permanently
stops on the first input-subsystem change, relevant logind signal, or evidence
loss. It never rearms a broker instance."#;

const LOGIN_SERVICE: &str = "org.freedesktop.login1";
const LOGIN_PATH: &str = "/org/freedesktop/login1";
const MANAGER_INTERFACE: &str = "org.freedesktop.login1.Manager";
const SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";
const SEAT_INTERFACE: &str = "org.freedesktop.login1.Seat";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";
const MAX_SESSION_ID_BYTES: usize = 64;
const MAX_SOCKET_PATH_BYTES: usize = 100;
const DBUS_TIMEOUT: Duration = Duration::from_secs(2);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const EVIDENCE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_UEVENT_BYTES: usize = 16 * 1024;
const MAX_UEVENT_FIELDS: usize = 128;
const SYSFS_INPUT: &str = "/sys/class/input";

fn main() -> ExitCode {
    match run(std::env::args_os()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agent-seat-eligibility-guard: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let Some(arguments) = Arguments::parse(arguments)? else {
        println!("{HELP}");
        return Ok(());
    };
    let mut inherited = receive_inherited_files(1, 1)
        .map_err(|error| format!("cannot receive inherited descriptors: {error}"))?;
    if inherited[0].name() != "initial-input-set" {
        return Err("inherited descriptor must be named initial-input-set".to_owned());
    }
    let expected_input_set =
        read_initial_input_set(inherited.remove(0).into_descriptor(), arguments.peer_uid)?;
    let uevents = UeventMonitor::open()?;
    let current_input_set = inspect_input_class(Path::new(SYSFS_INPUT))?;
    let input_set_matches = current_input_set == expected_input_set;
    let listener = ListenerSource::open(&arguments.endpoint)?;
    let mut stream = accept_peer(listener.listener(), arguments.peer_uid)?;
    stream
        .set_write_timeout(Some(WRITE_TIMEOUT))
        .map_err(|error| format!("cannot bound eligibility writes: {error}"))?;

    let connection = Connection::new_system()
        .map_err(|error| format!("cannot connect to the system bus: {error}"))?;
    let stop = Arc::new(AtomicU8::new(Stop::None.encode()));
    subscribe(&connection, Arc::clone(&stop))?;
    let eligibility = current_eligibility(&connection, &arguments)?;
    while connection
        .process(Duration::ZERO)
        .map_err(|error| format!("cannot synchronize logind evidence: {error}"))?
    {}
    if uevents.drain()? {
        Stop::Changed.record(&stop);
    }
    let eligibility = if input_set_matches && matches!(Stop::load(&stop), Stop::None) {
        eligibility
    } else {
        EligibilityState::Ineligible
    };
    write_eligibility(&mut stream, eligibility)
        .map_err(|_| "cannot write initial eligibility".to_owned())?;
    if matches!(eligibility, EligibilityState::Ineligible) {
        return Ok(());
    }

    loop {
        connection
            .process(EVIDENCE_POLL_INTERVAL)
            .map_err(|error| format!("required logind evidence was lost: {error}"))?;
        match uevents.drain() {
            Ok(true) => Stop::Changed.record(&stop),
            Ok(false) => {}
            Err(error) => {
                Stop::Lost.record(&stop);
                return Err(error);
            }
        }
        match Stop::load(&stop) {
            Stop::None => {}
            Stop::Changed => {
                return write_eligibility(&mut stream, EligibilityState::Ineligible)
                    .map_err(|_| "cannot write terminal eligibility".to_owned());
            }
            Stop::Lost => return Err("required logind evidence was lost".to_owned()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Arguments {
    session: String,
    uid: u32,
    seat: String,
    endpoint: Endpoint,
    peer_uid: u32,
}

impl Arguments {
    fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Option<Self>, String> {
        let mut arguments = arguments.into_iter();
        let _program = arguments.next();
        let mut session = None;
        let mut uid = None;
        let mut seat = None;
        let mut socket = None;
        let mut listen_stdin = false;
        let mut peer_uid = None;
        while let Some(argument) = arguments.next() {
            let argument = argument
                .to_str()
                .ok_or_else(|| "arguments must be valid UTF-8".to_owned())?;
            match argument {
                "-h" | "--help"
                    if session.is_none()
                        && uid.is_none()
                        && seat.is_none()
                        && socket.is_none()
                        && !listen_stdin
                        && peer_uid.is_none()
                        && arguments.next().is_none() =>
                {
                    return Ok(None);
                }
                "--session" if session.is_none() => {
                    session = Some(value(&mut arguments, "--session")?);
                }
                "--uid" if uid.is_none() => {
                    uid = Some(number(&mut arguments, "--uid")?);
                }
                "--seat" if seat.is_none() => {
                    seat = Some(value(&mut arguments, "--seat")?);
                }
                "--socket" if socket.is_none() => {
                    socket = Some(PathBuf::from(value(&mut arguments, "--socket")?));
                }
                "--listen-stdin" if !listen_stdin => listen_stdin = true,
                "--peer-uid" if peer_uid.is_none() => {
                    peer_uid = Some(number(&mut arguments, "--peer-uid")?);
                }
                "--session" | "--uid" | "--seat" | "--socket" | "--listen-stdin" | "--peer-uid" => {
                    return Err(format!("{argument} may be specified only once"));
                }
                _ => return Err(format!("unknown argument: {argument}")),
            }
        }
        let session = session.ok_or_else(|| "--session is required".to_owned())?;
        if session.is_empty()
            || session.len() > MAX_SESSION_ID_BYTES
            || !session
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        {
            return Err("--session is malformed".to_owned());
        }
        let seat = seat.ok_or_else(|| "--seat is required".to_owned())?;
        if seat != "seat0" {
            return Err("only seat0 is currently supported".to_owned());
        }
        let endpoint = match (socket, listen_stdin) {
            (Some(socket), false) => {
                if !socket.is_absolute()
                    || socket.as_os_str().as_encoded_bytes().len() > MAX_SOCKET_PATH_BYTES
                {
                    return Err("--socket must be an absolute bounded path".to_owned());
                }
                Endpoint::Path(socket)
            }
            (None, true) => Endpoint::StandardInput,
            (Some(_), true) => {
                return Err("--socket and --listen-stdin are mutually exclusive".to_owned());
            }
            (None, false) => {
                return Err("--socket or --listen-stdin is required".to_owned());
            }
        };
        Ok(Some(Self {
            session,
            uid: uid.ok_or_else(|| "--uid is required".to_owned())?,
            seat,
            endpoint,
            peer_uid: peer_uid.unwrap_or(0),
        }))
    }
}

fn read_initial_input_set(
    descriptor: OwnedFd,
    owner_uid: u32,
) -> Result<Vec<InputClassMapping>, String> {
    let mut file = File::from(descriptor);
    let before = file
        .metadata()
        .map_err(|error| format!("cannot inspect initial input-set descriptor: {error}"))?;
    if !before.file_type().is_file()
        || before.nlink() != 1
        || before.uid() != owner_uid
        || before.mode() & 0o777 != 0o600
        || before.len() > u64::try_from(MAX_INPUT_SET_BYTES).unwrap_or(u64::MAX)
    {
        return Err("initial input set is not an exact private peer-owned file".to_owned());
    }
    let mut source = Vec::with_capacity(usize::try_from(before.len()).unwrap_or(0));
    file.by_ref()
        .take(u64::try_from(MAX_INPUT_SET_BYTES).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut source)
        .map_err(|error| format!("cannot read initial input set: {error}"))?;
    if source.len() > MAX_INPUT_SET_BYTES {
        return Err("initial input set exceeds its bound".to_owned());
    }
    let after = file
        .metadata()
        .map_err(|error| format!("cannot recheck initial input set: {error}"))?;
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || !after.file_type().is_file()
        || after.nlink() != 1
        || after.uid() != owner_uid
        || after.mode() & 0o777 != 0o600
    {
        return Err("initial input set changed while it was read".to_owned());
    }
    decode_input_set(&source).map_err(|error| error.to_string())
}

fn inspect_input_class(directory: &Path) -> Result<Vec<InputClassMapping>, String> {
    let raw = inspect_input_class_entries(directory)?;
    let mappings = raw
        .into_iter()
        .map(|(number, path)| {
            InputClassMapping::new(number, path)
                .map_err(|error| format!("kernel input class mapping is invalid: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if mappings.is_empty() {
        return Err("kernel input class contains no event devices".to_owned());
    }
    Ok(mappings)
}

fn inspect_input_class_entries(directory: &Path) -> Result<Vec<(u32, PathBuf)>, String> {
    let mut raw = Vec::new();
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("cannot enumerate {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("cannot enumerate input class: {error}"))?;
        let name = entry.file_name();
        let bytes = name.as_encoded_bytes();
        if !bytes.starts_with(b"event") {
            continue;
        }
        let number = event_number(&name)
            .ok_or_else(|| "kernel input class contains a malformed event name".to_owned())?;
        if raw.len() >= MAX_INPUT_CLASS_MAPPINGS {
            return Err("kernel input class exceeds its mapping bound".to_owned());
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| format!("cannot inspect kernel input class entry: {error}"))?;
        if !metadata.file_type().is_symlink() {
            return Err("kernel input class event entry is not a symlink".to_owned());
        }
        let canonical = fs::canonicalize(entry.path())
            .map_err(|error| format!("cannot resolve kernel input class entry: {error}"))?;
        raw.push((number, canonical));
    }
    raw.sort_unstable_by_key(|(number, _)| *number);
    Ok(raw)
}

fn event_number(name: &std::ffi::OsStr) -> Option<u32> {
    let name = name.to_str()?;
    let value = name.strip_prefix("event")?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = value.parse::<u32>().ok()?;
    (name == format!("event{number}")).then_some(number)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Endpoint {
    Path(PathBuf),
    StandardInput,
}

fn value(arguments: &mut impl Iterator<Item = OsString>, option: &str) -> Result<String, String> {
    arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| format!("{option} requires a UTF-8 value"))
}

fn number(arguments: &mut impl Iterator<Item = OsString>, option: &str) -> Result<u32, String> {
    value(arguments, option)?
        .parse()
        .map_err(|_| format!("{option} requires an unsigned integer"))
}

struct UeventMonitor(OwnedFd);

impl UeventMonitor {
    fn open() -> Result<Self, String> {
        let socket = socket_with(
            AddressFamily::NETLINK,
            SocketType::DGRAM,
            SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            Some(netlink::KOBJECT_UEVENT),
        )
        .map_err(|error| format!("cannot open kernel device-change evidence: {error}"))?;
        bind(&socket, &netlink::SocketAddrNetlink::new(0, 1)).map_err(|error| {
            format!("cannot subscribe to kernel device-change evidence: {error}")
        })?;
        Ok(Self(socket))
    }

    fn drain(&self) -> Result<bool, String> {
        let mut changed = false;
        let mut buffer = [0_u8; MAX_UEVENT_BYTES];
        loop {
            match recvfrom(&self.0, &mut buffer, RecvFlags::TRUNC) {
                Ok((initialized, received, Some(source))) => {
                    if received > buffer.len() || initialized != received {
                        return Err("kernel device-change evidence was truncated".to_owned());
                    }
                    let source = netlink::SocketAddrNetlink::try_from(source).map_err(|_| {
                        "kernel device-change evidence has an invalid source".to_owned()
                    })?;
                    if source.pid() != 0 {
                        return Err(
                            "kernel device-change evidence came from a userspace sender".to_owned()
                        );
                    }
                    changed |= is_input_uevent(&buffer[..initialized])?;
                }
                Ok((_, _, None)) => {
                    return Err("kernel device-change evidence has no source".to_owned());
                }
                Err(error) if error == rustix::io::Errno::AGAIN => return Ok(changed),
                Err(error) => {
                    return Err(format!("kernel device-change evidence was lost: {error}"));
                }
            }
        }
    }
}

fn is_input_uevent(message: &[u8]) -> Result<bool, String> {
    if message.is_empty() || !message.ends_with(&[0]) {
        return Err("kernel device-change message is incomplete".to_owned());
    }
    let mut fields = message.split(|byte| *byte == 0);
    let header = fields
        .next()
        .ok_or_else(|| "kernel device-change message has no header".to_owned())?;
    let Some(separator) = header.iter().position(|byte| *byte == b'@') else {
        return Err("kernel device-change message header is malformed".to_owned());
    };
    let (action, path_with_separator) = header.split_at(separator);
    let path = &path_with_separator[1..];
    if action.is_empty() || !path.starts_with(b"/") {
        return Err("kernel device-change message header is malformed".to_owned());
    }

    let mut count = 1_usize;
    let mut subsystem = None;
    let mut ended = false;
    for field in fields {
        if field.is_empty() {
            ended = true;
            continue;
        }
        if ended {
            return Err("kernel device-change message has an interior terminator".to_owned());
        }
        count = count.saturating_add(1);
        if count > MAX_UEVENT_FIELDS {
            return Err("kernel device-change message has too many fields".to_owned());
        }
        if let Some(value) = field.strip_prefix(b"SUBSYSTEM=") {
            if value.is_empty() || subsystem.replace(value).is_some() {
                return Err(
                    "kernel device-change message has invalid subsystem evidence".to_owned(),
                );
            }
        }
    }
    let subsystem = subsystem
        .ok_or_else(|| "kernel device-change message has no subsystem evidence".to_owned())?;
    if subsystem == b"input" && !path.starts_with(b"/devices/") {
        return Err("kernel input-device change has an invalid device path".to_owned());
    }
    Ok(subsystem == b"input")
}

struct OwnedListener {
    listener: UnixListener,
    path: PathBuf,
    device: u64,
    inode: u64,
}

enum ListenerSource {
    Owned(OwnedListener),
    Inherited(UnixListener),
}

impl ListenerSource {
    fn open(endpoint: &Endpoint) -> Result<Self, String> {
        match endpoint {
            Endpoint::Path(path) => OwnedListener::bind(path).map(Self::Owned),
            Endpoint::StandardInput => {
                let descriptor = rustix::io::dup(std::io::stdin().as_fd())
                    .map_err(|error| format!("cannot duplicate inherited listener: {error}"))?;
                let listener = UnixListener::from(descriptor);
                listener.local_addr().map_err(|error| {
                    format!("standard input is not an AF_UNIX listener: {error}")
                })?;
                Ok(Self::Inherited(listener))
            }
        }
    }

    const fn listener(&self) -> &UnixListener {
        match self {
            Self::Owned(listener) => listener.listener(),
            Self::Inherited(listener) => listener,
        }
    }
}

impl OwnedListener {
    fn bind(path: &Path) -> Result<Self, String> {
        let listener = UnixListener::bind(path)
            .map_err(|error| format!("cannot bind {}: {error}", path.display()))?;
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        let owned = Self {
            listener,
            path: path.to_path_buf(),
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("cannot protect {}: {error}", path.display()))?;
        Ok(owned)
    }

    const fn listener(&self) -> &UnixListener {
        &self.listener
    }
}

impl Drop for OwnedListener {
    fn drop(&mut self) {
        let owned = fs::symlink_metadata(&self.path)
            .is_ok_and(|metadata| metadata.dev() == self.device && metadata.ino() == self.inode);
        if owned {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn accept_peer(listener: &UnixListener, expected_uid: u32) -> Result<UnixStream, String> {
    loop {
        let (stream, _) = listener
            .accept()
            .map_err(|error| format!("cannot accept eligibility consumer: {error}"))?;
        let peer = socket_peercred(&stream)
            .map_err(|error| format!("cannot authenticate eligibility consumer: {error}"))?;
        if peer.uid.as_raw() == expected_uid {
            return Ok(stream);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stop {
    None,
    Changed,
    Lost,
}

impl Stop {
    const fn encode(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Changed => 1,
            Self::Lost => 2,
        }
    }

    fn load(value: &AtomicU8) -> Self {
        match value.load(Ordering::Acquire) {
            0 => Self::None,
            1 => Self::Changed,
            _ => Self::Lost,
        }
    }

    fn record(self, value: &AtomicU8) {
        value.fetch_max(self.encode(), Ordering::AcqRel);
    }
}

fn subscribe(connection: &Connection, stop: Arc<AtomicU8>) -> Result<(), String> {
    let properties_rule = MatchRule::new_signal(PROPERTIES_INTERFACE, "PropertiesChanged")
        .with_sender(LOGIN_SERVICE)
        .with_namespaced_path(LOGIN_PATH)
        .static_clone();
    let property_stop = Arc::clone(&stop);
    connection
        .add_match(
            properties_rule,
            move |_: PropertiesPropertiesChanged, _, _| {
                Stop::Changed.record(&property_stop);
                true
            },
        )
        .map_err(dbus_error)?;

    for member in ["PrepareForSleep", "PrepareForShutdown"] {
        let rule = MatchRule::new_signal(MANAGER_INTERFACE, member)
            .with_sender(LOGIN_SERVICE)
            .with_path(LOGIN_PATH)
            .static_clone();
        let manager_stop = Arc::clone(&stop);
        connection
            .add_match(rule, move |_: (bool,), _, _| {
                Stop::Changed.record(&manager_stop);
                true
            })
            .map_err(dbus_error)?;
    }

    let owner_rule = MatchRule::new_signal(DBUS_INTERFACE, "NameOwnerChanged").static_clone();
    connection
        .add_match(
            owner_rule,
            move |(name, _, _): (String, String, String), _, _| {
                if name == LOGIN_SERVICE {
                    Stop::Lost.record(&stop);
                }
                true
            },
        )
        .map_err(dbus_error)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EligibilityFacts {
    owner_matches: bool,
    seat_matches: bool,
    foreground_session_matches: bool,
    session_active: bool,
    session_local: bool,
    session_x11: bool,
    session_user_class: bool,
    session_state_active: bool,
    session_unlocked: bool,
    system_awake: bool,
    system_not_shutting_down: bool,
}

impl EligibilityFacts {
    fn state(self) -> EligibilityState {
        if self.owner_matches
            && self.seat_matches
            && self.foreground_session_matches
            && self.session_active
            && self.session_local
            && self.session_x11
            && self.session_user_class
            && self.session_state_active
            && self.session_unlocked
            && self.system_awake
            && self.system_not_shutting_down
        {
            EligibilityState::Eligible
        } else {
            EligibilityState::Ineligible
        }
    }
}

fn current_eligibility(
    connection: &Connection,
    arguments: &Arguments,
) -> Result<EligibilityState, String> {
    let manager = connection.with_proxy(LOGIN_SERVICE, LOGIN_PATH, DBUS_TIMEOUT);
    let (session_path,): (DbusPath<'static>,) = manager
        .method_call(
            MANAGER_INTERFACE,
            "GetSession",
            (arguments.session.as_str(),),
        )
        .map_err(dbus_error)?;
    let session = connection.with_proxy(LOGIN_SERVICE, session_path.clone(), DBUS_TIMEOUT);
    let (session_uid, _): (u32, DbusPath<'static>) =
        session.get(SESSION_INTERFACE, "User").map_err(dbus_error)?;
    let (seat_name, seat_path): (String, DbusPath<'static>) =
        session.get(SESSION_INTERFACE, "Seat").map_err(dbus_error)?;
    let seat = connection.with_proxy(LOGIN_SERVICE, seat_path, DBUS_TIMEOUT);
    let (active_session, active_path): (String, DbusPath<'static>) = seat
        .get(SEAT_INTERFACE, "ActiveSession")
        .map_err(dbus_error)?;
    let facts = EligibilityFacts {
        owner_matches: session_uid == arguments.uid,
        seat_matches: seat_name == arguments.seat,
        foreground_session_matches: active_session == arguments.session
            && active_path == session_path,
        session_active: session
            .get::<bool>(SESSION_INTERFACE, "Active")
            .map_err(dbus_error)?,
        session_local: !session
            .get::<bool>(SESSION_INTERFACE, "Remote")
            .map_err(dbus_error)?,
        session_x11: session
            .get::<String>(SESSION_INTERFACE, "Type")
            .map_err(dbus_error)?
            == "x11",
        session_user_class: session
            .get::<String>(SESSION_INTERFACE, "Class")
            .map_err(dbus_error)?
            == "user",
        session_state_active: session
            .get::<String>(SESSION_INTERFACE, "State")
            .map_err(dbus_error)?
            == "active",
        session_unlocked: !session
            .get::<bool>(SESSION_INTERFACE, "LockedHint")
            .map_err(dbus_error)?,
        system_awake: !manager
            .get::<bool>(MANAGER_INTERFACE, "PreparingForSleep")
            .map_err(dbus_error)?,
        system_not_shutting_down: !manager
            .get::<bool>(MANAGER_INTERFACE, "PreparingForShutdown")
            .map_err(dbus_error)?,
    };
    Ok(facts.state())
}

fn dbus_error(error: impl std::fmt::Display) -> String {
    format!("logind evidence is unavailable: {error}")
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicU64, Ordering};

    use agent_seat_activity_broker::read_eligibility;
    use rustix::process::geteuid;

    use super::*;

    static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

    fn arguments(values: &[&str]) -> Result<Option<Arguments>, String> {
        Arguments::parse(values.iter().map(OsString::from))
    }

    #[test]
    fn arguments_are_exact_absolute_and_deny_unknown_values() {
        let parsed = arguments(&[
            "guard",
            "--session",
            "68",
            "--uid",
            "1000",
            "--seat",
            "seat0",
            "--socket",
            "/run/guard.sock",
        ])
        .expect("valid guard arguments")
        .expect("serve arguments");
        assert_eq!(parsed.session, "68");
        assert_eq!(parsed.uid, 1000);
        assert_eq!(parsed.peer_uid, 0);
        assert_eq!(
            parsed.endpoint,
            Endpoint::Path(PathBuf::from("/run/guard.sock"))
        );
        assert!(
            arguments(&["guard", "--help"])
                .expect("help arguments")
                .is_none()
        );
        let inherited = arguments(&[
            "guard",
            "--session",
            "68",
            "--uid",
            "1000",
            "--seat",
            "seat0",
            "--listen-stdin",
        ])
        .expect("inherited guard arguments")
        .expect("inherited endpoint");
        assert_eq!(inherited.endpoint, Endpoint::StandardInput);
        assert!(
            arguments(&[
                "guard",
                "--session",
                "68",
                "--uid",
                "1000",
                "--seat",
                "seat0",
                "--socket",
                "/run/x",
                "--listen-stdin",
            ])
            .is_err()
        );
        assert!(arguments(&["guard", "--session", "../68"]).is_err());
        assert!(
            arguments(&[
                "guard",
                "--session",
                "68",
                "--uid",
                "1000",
                "--seat",
                "seat1",
                "--socket",
                "/run/x",
            ])
            .is_err()
        );
        assert!(
            arguments(&[
                "guard",
                "--session",
                "68",
                "--uid",
                "1000",
                "--seat",
                "seat0",
                "--socket",
                "relative",
            ])
            .is_err()
        );
    }

    #[test]
    fn stop_state_is_monotonic_when_stored_by_callbacks() {
        let stop = AtomicU8::new(Stop::None.encode());
        assert_eq!(Stop::load(&stop), Stop::None);
        Stop::Changed.record(&stop);
        assert_eq!(Stop::load(&stop), Stop::Changed);
        Stop::Lost.record(&stop);
        assert_eq!(Stop::load(&stop), Stop::Lost);
        Stop::Changed.record(&stop);
        assert_eq!(Stop::load(&stop), Stop::Lost);
    }

    #[test]
    fn every_incomplete_session_or_system_fact_is_ineligible() {
        let eligible = EligibilityFacts {
            owner_matches: true,
            seat_matches: true,
            foreground_session_matches: true,
            session_active: true,
            session_local: true,
            session_x11: true,
            session_user_class: true,
            session_state_active: true,
            session_unlocked: true,
            system_awake: true,
            system_not_shutting_down: true,
        };
        assert_eq!(eligible.state(), EligibilityState::Eligible);

        for ineligible in [
            EligibilityFacts {
                owner_matches: false,
                ..eligible
            },
            EligibilityFacts {
                seat_matches: false,
                ..eligible
            },
            EligibilityFacts {
                foreground_session_matches: false,
                ..eligible
            },
            EligibilityFacts {
                session_active: false,
                ..eligible
            },
            EligibilityFacts {
                session_local: false,
                ..eligible
            },
            EligibilityFacts {
                session_x11: false,
                ..eligible
            },
            EligibilityFacts {
                session_user_class: false,
                ..eligible
            },
            EligibilityFacts {
                session_state_active: false,
                ..eligible
            },
            EligibilityFacts {
                session_unlocked: false,
                ..eligible
            },
            EligibilityFacts {
                system_awake: false,
                ..eligible
            },
            EligibilityFacts {
                system_not_shutting_down: false,
                ..eligible
            },
        ] {
            assert_eq!(ineligible.state(), EligibilityState::Ineligible);
        }
    }

    #[test]
    fn kernel_uevents_are_bounded_and_only_input_changes_stop() {
        assert_eq!(
            is_input_uevent(
                b"add@/devices/platform/example/input/input9/event9\0ACTION=add\0DEVPATH=/devices/platform/example/input/input9/event9\0SUBSYSTEM=input\0SEQNUM=42\0"
            ),
            Ok(true)
        );
        assert_eq!(
            is_input_uevent(
                b"change@/devices/pci0000:00/example\0ACTION=change\0SUBSYSTEM=pci\0SEQNUM=43\0"
            ),
            Ok(false)
        );
        for malformed in [
            &b"add@/devices/example\0SUBSYSTEM=input"[..],
            &b"add/devices/example\0SUBSYSTEM=input\0"[..],
            &b"add@/class/input/example\0SUBSYSTEM=input\0"[..],
            &b"add@/devices/example\0SUBSYSTEM=input\0SUBSYSTEM=input\0"[..],
            &b"add@/devices/example\0SUBSYSTEM=input\0\0SEQNUM=44\0"[..],
            &b"add@/devices/example\0ACTION=add\0"[..],
        ] {
            assert!(is_input_uevent(malformed).is_err());
        }

        let mut over_bound = b"add@/devices/example\0SUBSYSTEM=pci\0".to_vec();
        for _ in 0..MAX_UEVENT_FIELDS {
            over_bound.extend_from_slice(b"KEY=value\0");
        }
        assert!(is_input_uevent(&over_bound).is_err());
    }

    #[test]
    fn initial_input_set_descriptor_is_private_bounded_and_exact() {
        let path = std::env::temp_dir().join(format!(
            "agent-seat-input-set-{}-{}",
            std::process::id(),
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ));
        let mapping = InputClassMapping::new(
            2,
            PathBuf::from("/sys/devices/platform/example/input/input2/event2"),
        )
        .expect("mapping fixture");
        fs::write(
            &path,
            agent_seat_activity_broker::encode_input_set(std::slice::from_ref(&mapping))
                .expect("encoded mapping fixture"),
        )
        .expect("write input-set fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .expect("protect input-set fixture");
        let file = File::open(&path).expect("open input-set fixture");
        assert_eq!(
            read_initial_input_set(
                file.try_clone().expect("clone fixture").into(),
                geteuid().as_raw()
            ),
            Ok(vec![mapping])
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))
            .expect("weaken input-set fixture");
        assert!(
            read_initial_input_set(
                file.try_clone().expect("clone fixture").into(),
                geteuid().as_raw()
            )
            .is_err()
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .expect("restore input-set fixture");
        assert!(
            read_initial_input_set(
                file.try_clone().expect("clone fixture").into(),
                geteuid().as_raw().saturating_add(1)
            )
            .is_err()
        );
        fs::remove_file(path).expect("remove input-set fixture");
    }

    #[test]
    fn input_class_scan_requires_canonical_event_symlinks_and_sorts_them() {
        let root = std::env::temp_dir().join(format!(
            "agent-seat-input-class-{}-{}",
            std::process::id(),
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ));
        let class = root.join("class");
        let devices = root.join("devices");
        fs::create_dir_all(&class).expect("create class fixture");
        for number in [10, 2] {
            let target = devices.join(format!("input{number}/event{number}"));
            fs::create_dir_all(&target).expect("create device fixture");
            symlink(&target, class.join(format!("event{number}")))
                .expect("create class symlink fixture");
        }
        fs::write(class.join("mouse0"), []).expect("create irrelevant class fixture");
        let entries = inspect_input_class_entries(&class).expect("scan class fixture");
        assert_eq!(
            entries
                .iter()
                .map(|(number, _)| *number)
                .collect::<Vec<_>>(),
            [2, 10]
        );
        fs::write(class.join("event01"), []).expect("create malformed class fixture");
        assert!(inspect_input_class_entries(&class).is_err());
        fs::remove_dir_all(root).expect("remove class fixture");
    }

    #[test]
    fn socket_is_private_authenticated_and_removed_by_its_owner() {
        let path = std::env::temp_dir().join(format!(
            "agent-seat-eligibility-{}-{}.sock",
            std::process::id(),
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = OwnedListener::bind(&path).expect("bind guard fixture");
        let mode = fs::symlink_metadata(&path)
            .expect("guard socket metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let client = UnixStream::connect(&path).expect("connect guard fixture");
        let mut server = accept_peer(listener.listener(), geteuid().as_raw())
            .expect("authenticate guard fixture");
        write_eligibility(&mut server, EligibilityState::Eligible)
            .expect("write eligibility fixture");
        assert_eq!(
            read_eligibility(&mut &client),
            Ok(EligibilityState::Eligible)
        );
        drop(listener);
        assert!(!path.exists());
    }
}
