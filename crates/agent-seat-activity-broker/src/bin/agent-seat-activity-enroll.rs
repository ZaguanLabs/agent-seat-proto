//! Explicit administrator inspection and lifecycle transactions for the broker.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read as _, Write as _};
use std::os::unix::fs::{
    DirBuilderExt as _, FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _,
    PermissionsExt as _,
};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use agent_seat_activity_broker::{
    DeviceCapabilities, DeviceIdentity, EnrolledDevice, IdentityStrength, InputClassMapping,
    MAX_EVENT_DESCRIPTORS, MAX_INPUT_CLASS_MAPPINGS, encode_device_set, encode_input_set,
};
use rustix::fs::{
    AtFlags, Mode, OFlags, RenameFlags, fcntl_getfl, fcntl_setfl, openat, renameat_with, unlinkat,
};

const HELP: &str = r#"agent-seat-activity-enroll - inspect a physical-seat input candidate

USAGE:
  agent-seat-activity-enroll inspect [--seat seat0]
  agent-seat-activity-enroll render --uid UID --session ID --output ABSOLUTE-DIRECTORY
  agent-seat-activity-enroll verify --uid UID --session ID --bundle ABSOLUTE-DIRECTORY
  agent-seat-activity-enroll install --uid UID --session ID --bundle ABSOLUTE-DIRECTORY --confirm-install
  agent-seat-activity-enroll arm --uid UID --session ID --confirm-arm
  agent-seat-activity-enroll stop --uid UID
  agent-seat-activity-enroll purge --uid UID --confirm-purge
  agent-seat-activity-enroll --help

OPTIONS:
  --seat seat0       Inspect the only currently supported physical seat
  --uid UID          Numeric owner of the X11 session
  --session ID       Exact logind session identifier
  --output DIRECTORY New private directory for the review bundle
  --bundle DIRECTORY Existing private review bundle to verify
  --confirm-install  Confirm the explicit root-only file installation
  --confirm-arm      Confirm one explicit root-only arm cycle
  --confirm-purge    Confirm removal of the exact UID-bound installed files
  -h, --help         Print this help

Inspect, render, and verify are unprivileged and read only sysfs metadata, device-node
metadata, and selected udev properties. They do not open input devices, read
events, install units, start services, change permissions, or modify Agent
Seat policy. Render writes four inert unit files, complete input-class and
reviewed-device manifests, and a human-readable review record to a new
mode-0700 directory. It never overwrites an existing path.
Verify regenerates the expected bundle from the current seat and requires an
exact, private, direct-file match. It changes nothing.

Install is a separate root-only transaction. It re-verifies the current seat
and the UID-owned review bundle, requires root-owned packaged executables, and
creates new root-owned enrollment and unit files without overwrite. It never
enables, reloads, starts, stops, or rearms a service and never changes groups,
ACLs, udev rules, provider policy, or device permissions.

Arm is a separate root-only current-set verification and one service start. It
does not enable automatic startup. Stop terminates only the exact UID-bound
broker and guard units. Neither command changes enrollment or provider policy.
Purge first stops those units, removes only exact root-owned Agent Seat files,
and reloads the system manager. It does not remove another package's files.

These commands make the experimental deployment testable; they do not make
input a supported Agent Seat profile. The separate deployment, confinement,
session, lock, and hostile-test gates still apply."#;

const UDEVADM: &str = "/usr/bin/udevadm";
const SYSFS_INPUT: &str = "/sys/class/input";
const DEV_INPUT: &str = "/dev/input";
const SUPPORTED_SEAT: &str = "seat0";
const BROKER_EXEC: &str = "/usr/bin/agent-seat-activity-broker";
const GUARD_EXEC: &str = "/usr/bin/agent-seat-eligibility-guard";
const GUARD_USER: &str = "agent-seat-guard";
const SYSTEMD_UNIT_DIRECTORY: &str = "/etc/systemd/system";
const ENROLLMENT_BASE_DIRECTORY: &str = "/etc/agent-seat/activity";
const MAX_UDEV_OUTPUT_BYTES: usize = 16 * 1024;
const MAX_SYSFS_PATH_BYTES: usize = 4 * 1024;
const MAX_SYSFS_CAPABILITY_BYTES: usize = 4 * 1024;
const MAX_BUNDLE_FILE_BYTES: u64 = 64 * 1024;
const MAX_UDEV_QUERY_DURATION: Duration = Duration::from_secs(2);
// Longer than the broker's 250 ms initial quiet interval. A second active
// check catches setup failures that occur just after systemd reports start.
const ARM_STABILITY_INTERVAL: Duration = Duration::from_millis(750);
const PROPERTY_NAMES: &str = "ID_BUS,ID_INPUT,ID_INPUT_KEY,ID_INPUT_KEYBOARD,ID_INPUT_MOUSE,ID_INPUT_TOUCHPAD,ID_INPUT_TOUCHSCREEN,ID_INPUT_TABLET,ID_MODEL_ID,ID_PATH,ID_REVISION,ID_SEAT,ID_SERIAL_SHORT,ID_VENDOR_ID";
const BROKER_SERVICE_SOURCE: &str =
    include_str!("../../../../contrib/systemd/agent-seat-activity-broker.service.in");
const BROKER_SOCKET_SOURCE: &str =
    include_str!("../../../../contrib/systemd/agent-seat-activity-broker.socket.in");
const GUARD_SERVICE_SOURCE: &str =
    include_str!("../../../../contrib/systemd/agent-seat-eligibility-guard.service.in");
const GUARD_SOCKET_SOURCE: &str =
    include_str!("../../../../contrib/systemd/agent-seat-eligibility-guard.socket.in");

fn main() -> ExitCode {
    match run(std::env::args_os()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agent-seat-activity-enroll: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    match Options::parse(arguments)? {
        Options::Help => {
            println!("{HELP}");
            Ok(())
        }
        Options::Inspect => {
            let inspection = inspect_devices(
                Path::new(SYSFS_INPUT),
                Path::new(DEV_INPUT),
                Path::new(UDEVADM),
                SUPPORTED_SEAT,
            )?;
            let stdout = std::io::stdout();
            write_report(&mut stdout.lock(), SUPPORTED_SEAT, &inspection.devices)
        }
        Options::Render(options) => {
            let inspection = inspect_devices(
                Path::new(SYSFS_INPUT),
                Path::new(DEV_INPUT),
                Path::new(UDEVADM),
                SUPPORTED_SEAT,
            )?;
            render_bundle(&options, &inspection)
        }
        Options::Verify(options) => {
            let inspection = inspect_devices(
                Path::new(SYSFS_INPUT),
                Path::new(DEV_INPUT),
                Path::new(UDEVADM),
                SUPPORTED_SEAT,
            )?;
            verify_bundle(&options, &inspection)
        }
        Options::Install(options) => install(options),
        Options::Arm(options) => arm(options),
        Options::Stop(uid) => stop(uid),
        Options::Purge(uid) => purge(uid),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Options {
    Help,
    Inspect,
    Render(BundleOptions),
    Verify(BundleOptions),
    Install(BundleOptions),
    Arm(ArmOptions),
    Stop(u32),
    Purge(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BundleOptions {
    uid: u32,
    session: String,
    directory: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ArmOptions {
    uid: u32,
    session: String,
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut arguments = arguments.into_iter();
        let _program = arguments.next();
        let Some(command) = arguments.next() else {
            return Err("a command is required; use --help".to_owned());
        };
        match command.to_str() {
            Some("-h" | "--help") => {
                if arguments.next().is_some() {
                    return Err("--help does not accept arguments".to_owned());
                }
                Ok(Self::Help)
            }
            Some("inspect") => {
                let mut seat = None;
                while let Some(argument) = arguments.next() {
                    match argument.to_str() {
                        Some("--seat") if seat.is_none() => {
                            let value = arguments
                                .next()
                                .and_then(|value| value.into_string().ok())
                                .ok_or_else(|| "--seat requires seat0".to_owned())?;
                            seat = Some(value);
                        }
                        Some("--seat") => {
                            return Err("--seat may be specified only once".to_owned());
                        }
                        Some(value) => return Err(format!("unknown argument: {value}")),
                        None => return Err("arguments must be valid UTF-8".to_owned()),
                    }
                }
                if seat.as_deref().unwrap_or(SUPPORTED_SEAT) != SUPPORTED_SEAT {
                    return Err("only seat0 is currently supported".to_owned());
                }
                Ok(Self::Inspect)
            }
            Some("render") => parse_bundle_options(arguments, "--output").map(Self::Render),
            Some("verify") => parse_bundle_options(arguments, "--bundle").map(Self::Verify),
            Some("install") => parse_install_options(arguments).map(Self::Install),
            Some("arm") => parse_arm_options(arguments).map(Self::Arm),
            Some("stop") => parse_stop_options(arguments).map(Self::Stop),
            Some("purge") => parse_confirmed_uid(arguments, "--confirm-purge").map(Self::Purge),
            Some(value) => Err(format!("unknown command: {value}")),
            None => Err("arguments must be valid UTF-8".to_owned()),
        }
    }
}

fn parse_arm_options(arguments: impl Iterator<Item = OsString>) -> Result<ArmOptions, String> {
    let mut confirmed = false;
    let mut uid = None;
    let mut session = None;
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--confirm-arm") if !confirmed => confirmed = true,
            Some("--uid") if uid.is_none() => uid = Some(parse_nonzero_uid(arguments.next())?),
            Some("--session") if session.is_none() => {
                session = Some(parse_session(arguments.next())?)
            }
            Some("--confirm-arm" | "--uid" | "--session") => {
                return Err(format!(
                    "{} may be specified only once",
                    argument.to_string_lossy()
                ));
            }
            Some(value) => return Err(format!("unknown argument: {value}")),
            None => return Err("arguments must be valid UTF-8".to_owned()),
        }
    }
    if !confirmed {
        return Err("arm requires --confirm-arm".to_owned());
    }
    Ok(ArmOptions {
        uid: uid.ok_or_else(|| "--uid is required".to_owned())?,
        session: session.ok_or_else(|| "--session is required".to_owned())?,
    })
}

fn parse_stop_options(mut arguments: impl Iterator<Item = OsString>) -> Result<u32, String> {
    let Some(option) = arguments.next() else {
        return Err("stop requires --uid".to_owned());
    };
    if option != OsStr::new("--uid") {
        return Err("stop accepts only --uid UID".to_owned());
    }
    let uid = parse_nonzero_uid(arguments.next())?;
    if arguments.next().is_some() {
        return Err("stop accepts only --uid UID".to_owned());
    }
    Ok(uid)
}

fn parse_confirmed_uid(
    arguments: impl Iterator<Item = OsString>,
    confirmation: &str,
) -> Result<u32, String> {
    let mut confirmed = false;
    let mut uid = None;
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some(value) if value == confirmation && !confirmed => confirmed = true,
            Some("--uid") if uid.is_none() => uid = Some(parse_nonzero_uid(arguments.next())?),
            Some(value) if value == confirmation || value == "--uid" => {
                return Err(format!("{value} may be specified only once"));
            }
            Some(value) => return Err(format!("unknown argument: {value}")),
            None => return Err("arguments must be valid UTF-8".to_owned()),
        }
    }
    if !confirmed {
        return Err(format!("operation requires {confirmation}"));
    }
    uid.ok_or_else(|| "--uid is required".to_owned())
}

fn parse_nonzero_uid(value: Option<OsString>) -> Result<u32, String> {
    let value = utf8_value(value, "--uid")?
        .parse::<u32>()
        .map_err(|_| "--uid requires a nonzero numeric UID".to_owned())?;
    if value == 0 {
        return Err("--uid requires a nonzero numeric UID".to_owned());
    }
    Ok(value)
}

fn parse_session(value: Option<OsString>) -> Result<String, String> {
    let value = utf8_value(value, "--session")?;
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    {
        return Err("--session is malformed".to_owned());
    }
    Ok(value)
}

fn parse_install_options(
    arguments: impl Iterator<Item = OsString>,
) -> Result<BundleOptions, String> {
    let mut confirmed = false;
    let mut remaining = Vec::new();
    for argument in arguments {
        if argument == OsStr::new("--confirm-install") {
            if confirmed {
                return Err("--confirm-install may be specified only once".to_owned());
            }
            confirmed = true;
        } else {
            remaining.push(argument);
        }
    }
    if !confirmed {
        return Err("install requires --confirm-install".to_owned());
    }
    parse_bundle_options(remaining.into_iter(), "--bundle")
}

fn parse_bundle_options(
    mut arguments: impl Iterator<Item = OsString>,
    directory_option: &str,
) -> Result<BundleOptions, String> {
    let mut uid = None;
    let mut session = None;
    let mut directory = None;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--uid") if uid.is_none() => {
                let value = utf8_value(arguments.next(), "--uid")?;
                let value = value
                    .parse::<u32>()
                    .map_err(|_| "--uid requires a nonzero numeric UID".to_owned())?;
                if value == 0 {
                    return Err("--uid requires a nonzero numeric UID".to_owned());
                }
                uid = Some(value);
            }
            Some("--session") if session.is_none() => {
                let value = utf8_value(arguments.next(), "--session")?;
                if value.is_empty()
                    || value.len() > 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
                {
                    return Err("--session is malformed".to_owned());
                }
                session = Some(value);
            }
            Some(value) if value == directory_option && directory.is_none() => {
                let value = arguments
                    .next()
                    .ok_or_else(|| format!("{directory_option} requires an absolute directory"))?;
                let value = PathBuf::from(value);
                if !is_safe_absolute_path(&value) {
                    return Err(format!(
                        "{directory_option} requires a normalized absolute directory"
                    ));
                }
                directory = Some(value);
            }
            Some("--uid" | "--session") => {
                return Err(format!(
                    "{} may be specified only once",
                    argument.to_string_lossy()
                ));
            }
            Some(value) if value == directory_option => {
                return Err(format!("{directory_option} may be specified only once"));
            }
            Some(value) => return Err(format!("unknown argument: {value}")),
            None => return Err("arguments must be valid UTF-8 except for --output".to_owned()),
        }
    }
    Ok(BundleOptions {
        uid: uid.ok_or_else(|| "--uid is required".to_owned())?,
        session: session.ok_or_else(|| "--session is required".to_owned())?,
        directory: directory.ok_or_else(|| format!("{directory_option} is required"))?,
    })
}

fn utf8_value(value: Option<OsString>, option: &str) -> Result<String, String> {
    value
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| format!("{option} requires a valid UTF-8 value"))
}

fn is_safe_absolute_path(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InspectedDevice {
    event_number: u32,
    device_node: PathBuf,
    sysfs_path: PathBuf,
    classes: Vec<&'static str>,
    identity: DeviceIdentity,
    capabilities: DeviceCapabilities,
    device_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InspectedInputSet {
    mappings: Vec<InputClassMapping>,
    devices: Vec<InspectedDevice>,
}

struct ObservedInputEvent {
    mapping: InputClassMapping,
    device_node: PathBuf,
    device_id: u64,
}

fn inspect_devices(
    sysfs_input: &Path,
    dev_input: &Path,
    udevadm: &Path,
    seat: &str,
) -> Result<InspectedInputSet, String> {
    let mut entries = fs::read_dir(sysfs_input)
        .map_err(|error| format!("cannot enumerate {}: {error}", sysfs_input.display()))?
        .filter_map(|entry| match entry {
            Ok(entry) => event_number(&entry.file_name()).map(|number| Ok((number, entry.path()))),
            Err(error) => Some(Err(format!("cannot enumerate input devices: {error}"))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if entries.len() > MAX_INPUT_CLASS_MAPPINGS {
        return Err(format!(
            "input device count exceeds the {MAX_INPUT_CLASS_MAPPINGS}-device scan bound"
        ));
    }
    sort_event_entries(&mut entries);

    let mut devices = Vec::new();
    let mut observed = Vec::with_capacity(entries.len());
    for (event_number, class_path) in entries {
        let event_name = format!("event{event_number}");
        let device_node = dev_input.join(&event_name);
        let metadata = fs::symlink_metadata(&device_node).map_err(|error| {
            format!(
                "device node {} disappeared during inspection: {error}",
                device_node.display()
            )
        })?;
        if !metadata.file_type().is_char_device() {
            return Err(format!(
                "{} is not a direct character device",
                device_node.display()
            ));
        }
        let canonical_class = fs::canonicalize(&class_path).map_err(|error| {
            format!(
                "cannot resolve input sysfs path {}: {error}",
                class_path.display()
            )
        })?;
        let properties = query_properties(udevadm, &device_node)?;
        let capabilities = read_device_capabilities(&class_path)?;
        let reported_sysfs = query_sysfs_path(udevadm, &device_node)?;
        if canonical_class != reported_sysfs {
            return Err(format!(
                "sysfs and udev identity disagree for {}",
                device_node.display()
            ));
        }
        let mapping = InputClassMapping::new(event_number, reported_sysfs.clone())
            .map_err(|error| format!("invalid input class mapping: {error}"))?;
        let classes = properties.relevant_classes()?;
        if properties.seat()? == seat && !classes.is_empty() {
            if devices.len() >= MAX_EVENT_DESCRIPTORS {
                return Err(format!(
                    "relevant device count exceeds the {MAX_EVENT_DESCRIPTORS}-device runtime bound"
                ));
            }
            devices.push(InspectedDevice {
                event_number,
                device_node: device_node.clone(),
                sysfs_path: reported_sysfs,
                classes,
                identity: properties.identity()?,
                capabilities,
                device_id: metadata.rdev(),
            });
        }
        observed.push(ObservedInputEvent {
            mapping,
            device_node,
            device_id: metadata.rdev(),
        });
    }
    if devices.is_empty() {
        return Err(format!("no relevant input devices were found for {seat}"));
    }

    for event in &observed {
        let metadata = fs::symlink_metadata(&event.device_node).map_err(|error| {
            format!(
                "device node {} disappeared during inspection: {error}",
                event.device_node.display()
            )
        })?;
        let class_path = sysfs_input.join(format!("event{}", event.mapping.event_number()));
        let canonical_class = fs::canonicalize(&class_path).map_err(|error| {
            format!(
                "input sysfs path {} changed during inspection: {error}",
                class_path.display()
            )
        })?;
        if !metadata.file_type().is_char_device()
            || metadata.rdev() != event.device_id
            || canonical_class != event.mapping.sysfs_path()
        {
            return Err(format!(
                "device identity changed during inspection: {}",
                event.device_node.display()
            ));
        }
    }
    for device in &devices {
        let properties = query_properties(udevadm, &device.device_node)?;
        let class_path = sysfs_input.join(format!("event{}", device.event_number));
        if properties.seat()? != seat
            || properties.relevant_classes()? != device.classes
            || properties.identity()? != device.identity
            || read_device_capabilities(&class_path)? != device.capabilities
        {
            return Err(format!(
                "relevant device evidence changed during inspection: {}",
                device.device_node.display()
            ));
        }
    }
    Ok(InspectedInputSet {
        mappings: observed.into_iter().map(|event| event.mapping).collect(),
        devices,
    })
}

fn sort_event_entries(entries: &mut [(u32, PathBuf)]) {
    entries.sort_unstable_by_key(|(number, _)| *number);
}

fn event_number(name: &OsStr) -> Option<u32> {
    let name = name.to_str()?;
    let number = name.strip_prefix("event")?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = number.parse::<u32>().ok()?;
    (parsed.to_string() == number).then_some(parsed)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Properties(BTreeMap<String, String>);

impl Properties {
    fn parse(output: &[u8]) -> Result<Self, String> {
        if output.len() > MAX_UDEV_OUTPUT_BYTES {
            return Err("udev property output exceeds its bound".to_owned());
        }
        let output = std::str::from_utf8(output)
            .map_err(|_| "udev property output is not UTF-8".to_owned())?;
        let mut properties = BTreeMap::new();
        for line in output.lines() {
            if line.is_empty() {
                continue;
            }
            let (name, value) = line
                .split_once('=')
                .ok_or_else(|| "udev property output is malformed".to_owned())?;
            if !PROPERTY_NAMES.split(',').any(|allowed| allowed == name) {
                return Err(format!("udev returned unexpected property {name:?}"));
            }
            if properties
                .insert(name.to_owned(), value.to_owned())
                .is_some()
            {
                return Err(format!("udev returned duplicate property {name:?}"));
            }
        }
        Ok(Self(properties))
    }

    fn flag(&self, name: &str) -> Result<bool, String> {
        match self.0.get(name).map(String::as_str) {
            None => Ok(false),
            Some("1") => Ok(true),
            Some(_) => Err(format!("udev property {name} has an unexpected value")),
        }
    }

    fn seat(&self) -> Result<&str, String> {
        let seat = self
            .0
            .get("ID_SEAT")
            .map(String::as_str)
            .unwrap_or(SUPPORTED_SEAT);
        if seat.is_empty()
            || seat.len() > 64
            || !seat
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        {
            return Err("udev ID_SEAT is malformed".to_owned());
        }
        Ok(seat)
    }

    fn relevant_classes(&self) -> Result<Vec<&'static str>, String> {
        if !self.flag("ID_INPUT")? {
            return Ok(Vec::new());
        }
        let mut classes = Vec::with_capacity(7);
        for (property, label) in [
            ("ID_INPUT_KEY", "key"),
            ("ID_INPUT_KEYBOARD", "keyboard"),
            ("ID_INPUT_MOUSE", "mouse"),
            ("ID_INPUT_TOUCHPAD", "touchpad"),
            ("ID_INPUT_TOUCHSCREEN", "touchscreen"),
            ("ID_INPUT_TABLET", "tablet"),
        ] {
            if self.flag(property)? {
                classes.push(label);
            }
        }
        Ok(classes)
    }

    fn identity(&self) -> Result<DeviceIdentity, String> {
        let path =
            self.0.get("ID_PATH").cloned().ok_or_else(|| {
                "udev ID_PATH is required for relevant-device identity".to_owned()
            })?;
        let selected = |name: &str| self.0.get(name).cloned();
        DeviceIdentity::new(
            path,
            selected("ID_BUS"),
            selected("ID_VENDOR_ID"),
            selected("ID_MODEL_ID"),
            selected("ID_REVISION"),
            selected("ID_SERIAL_SHORT"),
        )
        .map_err(|error| format!("udev relevant-device identity is malformed: {error}"))
    }
}

fn query_properties(udevadm: &Path, device: &Path) -> Result<Properties, String> {
    let output = run_udevadm(
        udevadm,
        [
            OsStr::new("info"),
            OsStr::new("--query=property"),
            OsStr::new("--property"),
            OsStr::new(PROPERTY_NAMES),
            OsStr::new("--name"),
            device.as_os_str(),
        ],
    )?;
    Properties::parse(&output)
}

fn query_sysfs_path(udevadm: &Path, device: &Path) -> Result<PathBuf, String> {
    let output = run_udevadm(
        udevadm,
        [
            OsStr::new("info"),
            OsStr::new("--query=path"),
            OsStr::new("--name"),
            device.as_os_str(),
        ],
    )?;
    parse_sysfs_path(&output)
}

fn read_device_capabilities(class_path: &Path) -> Result<DeviceCapabilities, String> {
    let device_path = class_path.join("device");
    let read = |relative: &str| read_sysfs_value(&device_path.join(relative));
    let values = [
        read("capabilities/abs")?,
        read("capabilities/ev")?,
        read("capabilities/ff")?,
        read("capabilities/key")?,
        read("capabilities/led")?,
        read("capabilities/msc")?,
        read("capabilities/rel")?,
        read("capabilities/snd")?,
        read("capabilities/sw")?,
        read("properties")?,
    ];
    DeviceCapabilities::new(values)
        .map_err(|error| format!("kernel input capabilities are malformed: {error}"))
}

fn read_sysfs_value(path: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() {
        return Err(format!(
            "{} is not a direct sysfs attribute",
            path.display()
        ));
    }
    let mut bytes = Vec::with_capacity(256);
    fs::File::open(path)
        .and_then(|file| {
            file.take(u64::try_from(MAX_SYSFS_CAPABILITY_BYTES + 1).unwrap_or(u64::MAX))
                .read_to_end(&mut bytes)
        })
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.len() > MAX_SYSFS_CAPABILITY_BYTES {
        return Err(format!("{} exceeds its read bound", path.display()));
    }
    let source =
        std::str::from_utf8(&bytes).map_err(|_| format!("{} is not UTF-8", path.display()))?;
    let value = source.strip_suffix('\n').unwrap_or(source);
    if value.contains('\n') {
        return Err(format!("{} contains multiple lines", path.display()));
    }
    Ok(value.to_owned())
}

fn run_udevadm<'a>(
    executable: &Path,
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> Result<Vec<u8>, String> {
    let mut child = Command::new(executable)
        .args(arguments)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot execute {}: {error}", executable.display()))?;
    let Some(mut stdout) = child.stdout.take() else {
        stop_child(&mut child);
        return Err(format!("cannot read {} output", executable.display()));
    };
    let flags = match fcntl_getfl(&stdout) {
        Ok(flags) => flags,
        Err(error) => {
            stop_child(&mut child);
            return Err(format!(
                "cannot bound {} output: {error}",
                executable.display()
            ));
        }
    };
    if let Err(error) = fcntl_setfl(&stdout, flags | OFlags::NONBLOCK) {
        stop_child(&mut child);
        return Err(format!(
            "cannot bound {} output: {error}",
            executable.display()
        ));
    }

    let deadline = Instant::now() + MAX_UDEV_QUERY_DURATION;
    let mut output = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stdout.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if output.len().saturating_add(count) > MAX_UDEV_OUTPUT_BYTES {
                    stop_child(&mut child);
                    return Err(format!("{} output exceeds its bound", executable.display()));
                }
                output.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    stop_child(&mut child);
                    return Err(format!(
                        "{} query exceeded its time bound",
                        executable.display()
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                stop_child(&mut child);
                return Err(format!(
                    "cannot read {} output: {error}",
                    executable.display()
                ));
            }
        }
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                stop_child(&mut child);
                return Err(format!(
                    "{} query exceeded its time bound",
                    executable.display()
                ));
            }
            Err(error) => {
                stop_child(&mut child);
                return Err(format!("cannot wait for {}: {error}", executable.display()));
            }
        }
    };
    if !status.success() {
        return Err(format!(
            "{} refused an input device query",
            executable.display()
        ));
    }
    Ok(output)
}

fn stop_child(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn parse_sysfs_path(output: &[u8]) -> Result<PathBuf, String> {
    if output.len() > MAX_SYSFS_PATH_BYTES {
        return Err("udev sysfs path exceeds its bound".to_owned());
    }
    let value =
        std::str::from_utf8(output).map_err(|_| "udev sysfs path is not UTF-8".to_owned())?;
    let value = value.strip_suffix('\n').unwrap_or(value);
    if value.is_empty() || value.contains('\n') || !value.starts_with("/devices/") {
        return Err("udev sysfs path is malformed".to_owned());
    }
    let relative = Path::new(value)
        .strip_prefix("/")
        .map_err(|_| "udev sysfs path is malformed".to_owned())?;
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("udev sysfs path is malformed".to_owned());
    }
    Ok(Path::new("/sys").join(relative))
}

fn write_report(
    output: &mut impl std::io::Write,
    seat: &str,
    devices: &[InspectedDevice],
) -> Result<(), String> {
    writeln!(output, "Agent Seat physical-input candidate")
        .and_then(|()| writeln!(output, "seat={seat}"))
        .and_then(|()| writeln!(output, "device_count={}", devices.len()))
        .map_err(|error| format!("cannot write inspection report: {error}"))?;
    for device in devices {
        writeln!(output)
            .and_then(|()| writeln!(output, "device={}", device.device_node.display()))
            .and_then(|()| writeln!(output, "sysfs={}", device.sysfs_path.display()))
            .and_then(|()| writeln!(output, "classes={}", device.classes.join(",")))
            .and_then(|()| {
                writeln!(
                    output,
                    "identity_strength={}",
                    identity_strength(device.identity.strength())
                )
            })
            .and_then(|()| writeln!(output, "coverage_evidence=topology+capabilities"))
            .map_err(|error| format!("cannot write inspection report: {error}"))?;
    }
    writeln!(output)
        .and_then(|()| writeln!(output, "review_only=true"))
        .and_then(|()| {
            writeln!(
                output,
                "No input device was opened and no enrollment was written."
            )
        })
        .map_err(|error| format!("cannot write inspection report: {error}"))
}

fn build_bundle(
    options: &BundleOptions,
    inspection: &InspectedInputSet,
) -> Result<RenderedBundle, String> {
    let devices = &inspection.devices;
    if devices.is_empty() || devices.len() > MAX_EVENT_DESCRIPTORS {
        return Err("render device set is outside the runtime bound".to_owned());
    }
    let stem = format!("agent-seat-activity-broker-{}", options.uid);
    let guard_stem = format!("agent-seat-eligibility-guard-{}", options.uid);
    let broker_service_name = format!("{stem}.service");
    let broker_socket_name = format!("{stem}.socket");
    let guard_service_name = format!("{guard_stem}.service");
    let guard_socket_name = format!("{guard_stem}.socket");
    let runtime_directory = format!("/run/agent-seat/{}", options.uid);
    let broker_socket_path = format!("{runtime_directory}/activity.sock");
    let eligibility_socket_path = format!("{runtime_directory}/eligibility.sock");
    let installed_input_set_path = format!(
        "/etc/agent-seat/activity/{}/initial-input-set.v1",
        options.uid
    );
    let installed_device_set_path = format!(
        "/etc/agent-seat/activity/{}/enrolled-device-set.v1",
        options.uid
    );
    let provider_uid = options.uid.to_string();

    let event_open_files = devices
        .iter()
        .enumerate()
        .map(|(index, device)| {
            format!(
                "OpenFile={}:event{index}:read-only",
                device.device_node.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let event_device_allow = devices
        .iter()
        .map(|device| format!("DeviceAllow={} r", device.device_node.display()))
        .collect::<Vec<_>>()
        .join("\n");

    let broker_service = BROKER_SERVICE_SOURCE
        .replace("@SOCKET_UNIT@", &broker_socket_name)
        .replace("@ELIGIBILITY_SOCKET_UNIT@", &guard_socket_name)
        .replace("@BROKER_EXEC@", BROKER_EXEC)
        .replace("@PROVIDER_UID@", &provider_uid)
        .replace("@ELIGIBILITY_PATH@", &eligibility_socket_path)
        .replace("@DEVICE_SET_PATH@", &installed_device_set_path)
        .replace("@EVENT_OPEN_FILES@", &event_open_files)
        .replace("@EVENT_DEVICE_ALLOW@", &event_device_allow);
    let broker_socket = BROKER_SOCKET_SOURCE
        .replace("@BROKER_SOCKET@", &broker_socket_path)
        .replace("@PROVIDER_UID@", &provider_uid);
    let guard_service = GUARD_SERVICE_SOURCE
        .replace("@ELIGIBILITY_SOCKET_UNIT@", &guard_socket_name)
        .replace("@GUARD_EXEC@", GUARD_EXEC)
        .replace("@GUARD_USER@", GUARD_USER)
        .replace("@SESSION_ID@", &options.session)
        .replace("@PROVIDER_UID@", &provider_uid)
        .replace("@INPUT_SET_PATH@", &installed_input_set_path);
    let guard_socket = GUARD_SOCKET_SOURCE
        .replace("@ELIGIBILITY_SOCKET@", &eligibility_socket_path)
        .replace("@ELIGIBILITY_SERVICE_UNIT@", &guard_service_name);

    for (name, source) in [
        (&broker_service_name, broker_service.as_str()),
        (&broker_socket_name, broker_socket.as_str()),
        (&guard_service_name, guard_service.as_str()),
        (&guard_socket_name, guard_socket.as_str()),
    ] {
        if [
            "@SOCKET_UNIT@",
            "@ELIGIBILITY_SOCKET_UNIT@",
            "@BROKER_EXEC@",
            "@PROVIDER_UID@",
            "@ELIGIBILITY_PATH@",
            "@EVENT_OPEN_FILES@",
            "@EVENT_DEVICE_ALLOW@",
            "@DEVICE_SET_PATH@",
            "@BROKER_SOCKET@",
            "@GUARD_EXEC@",
            "@GUARD_USER@",
            "@SESSION_ID@",
            "@INPUT_SET_PATH@",
            "@ELIGIBILITY_SOCKET@",
            "@ELIGIBILITY_SERVICE_UNIT@",
        ]
        .iter()
        .any(|marker| source.contains(marker))
        {
            return Err(format!(
                "rendered unit {name} contains an unresolved marker"
            ));
        }
    }

    let review = render_review(
        options,
        inspection,
        &installed_input_set_path,
        &broker_service_name,
        &broker_socket_name,
        &guard_service_name,
        &guard_socket_name,
    )?;
    let input_set = encode_input_set(&inspection.mappings)
        .map_err(|error| format!("cannot render initial input set: {error}"))?;
    let enrolled_devices = devices
        .iter()
        .map(|device| {
            EnrolledDevice::new(
                device.event_number,
                device.sysfs_path.clone(),
                device
                    .classes
                    .iter()
                    .map(|class| (*class).to_owned())
                    .collect(),
                device.identity.clone(),
                device.capabilities.clone(),
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot render reviewed device set: {error}"))?;
    let device_set = encode_device_set(&enrolled_devices)
        .map_err(|error| format!("cannot render reviewed device set: {error}"))?;
    Ok(RenderedBundle {
        files: vec![
            RenderedFile::new(broker_service_name, broker_service.into_bytes()),
            RenderedFile::new(broker_socket_name, broker_socket.into_bytes()),
            RenderedFile::new(guard_service_name, guard_service.into_bytes()),
            RenderedFile::new(guard_socket_name, guard_socket.into_bytes()),
            RenderedFile::new("initial-input-set.v1".to_owned(), input_set),
            RenderedFile::new("enrolled-device-set.v1".to_owned(), device_set),
            RenderedFile::new("REVIEW.txt".to_owned(), review),
        ],
    })
}

fn render_bundle(options: &BundleOptions, inspection: &InspectedInputSet) -> Result<(), String> {
    let bundle = build_bundle(options, inspection)?;
    let mut directory = NewBundleDirectory::create(&options.directory)?;
    for file in &bundle.files {
        directory.write(&file.name, &file.contents)?;
    }
    directory.finish();
    println!(
        "Rendered inert review bundle: {}",
        options.directory.display()
    );
    println!("Read REVIEW.txt before taking any administrative action.");
    println!("No unit was installed, enabled, or started.");
    Ok(())
}

fn render_review(
    options: &BundleOptions,
    inspection: &InspectedInputSet,
    installed_input_set_path: &str,
    broker_service: &str,
    broker_socket: &str,
    guard_service: &str,
    guard_socket: &str,
) -> Result<Vec<u8>, String> {
    let devices = &inspection.devices;
    let mut output = Vec::new();
    writeln!(output, "AGENT SEAT EXPERIMENTAL INPUT REVIEW BUNDLE")
        .and_then(|()| writeln!(output))
        .and_then(|()| writeln!(output, "review_only=true"))
        .and_then(|()| writeln!(output, "supported=false"))
        .and_then(|()| writeln!(output, "seat={SUPPORTED_SEAT}"))
        .and_then(|()| writeln!(output, "provider_uid={}", options.uid))
        .and_then(|()| writeln!(output, "session={}", options.session))
        .and_then(|()| writeln!(output, "device_count={}", devices.len()))
        .and_then(|()| {
            writeln!(
                output,
                "complete_input_class_count={}",
                inspection.mappings.len()
            )
        })
        .and_then(|()| writeln!(output, "input_set_file=initial-input-set.v1"))
        .and_then(|()| writeln!(output, "device_set_file=enrolled-device-set.v1"))
        .and_then(|()| writeln!(output, "installed_input_set={installed_input_set_path}"))
        .and_then(|()| writeln!(output))
        .and_then(|()| {
            writeln!(
                output,
                "This directory was generated without elevated privilege. It was not"
            )
        })
        .and_then(|()| {
            writeln!(
                output,
                "installed, enabled, or started. Generic Openbox input remains gated on"
            )
        })
        .and_then(|()| {
            writeln!(
                output,
                "a trusted lock transition and the documented hostile deployment tests."
            )
        })
        .and_then(|()| writeln!(output))
        .and_then(|()| writeln!(output, "units:"))
        .and_then(|()| writeln!(output, "  {broker_service}"))
        .and_then(|()| writeln!(output, "  {broker_socket}"))
        .and_then(|()| writeln!(output, "  {guard_service}"))
        .and_then(|()| writeln!(output, "  {guard_socket}"))
        .map_err(|error| format!("cannot render review record: {error}"))?;
    for device in devices {
        writeln!(output)
            .and_then(|()| writeln!(output, "device={}", device.device_node.display()))
            .and_then(|()| writeln!(output, "sysfs={}", device.sysfs_path.display()))
            .and_then(|()| writeln!(output, "classes={}", device.classes.join(",")))
            .and_then(|()| {
                writeln!(
                    output,
                    "identity_strength={}",
                    identity_strength(device.identity.strength())
                )
            })
            .and_then(|()| writeln!(output, "coverage_evidence=topology+capabilities"))
            .map_err(|error| format!("cannot render review record: {error}"))?;
    }
    writeln!(output)
        .and_then(|()| {
            writeln!(
                output,
                "The desktop user and dynamic broker require no input-group membership."
            )
        })
        .and_then(|()| {
            writeln!(
                output,
                "PID 1 would open only the listed event nodes and pass read-only FDs."
            )
        })
        .map_err(|error| format!("cannot render review record: {error}"))?;
    Ok(output)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RenderedFile {
    name: String,
    contents: Vec<u8>,
}

impl RenderedFile {
    fn new(name: String, contents: Vec<u8>) -> Self {
        Self { name, contents }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RenderedBundle {
    files: Vec<RenderedFile>,
}

const fn identity_strength(strength: IdentityStrength) -> &'static str {
    match strength {
        IdentityStrength::Topology => "topology",
        IdentityStrength::Serial => "serial",
    }
}

fn verify_bundle(options: &BundleOptions, inspection: &InspectedInputSet) -> Result<(), String> {
    let owner_uid = rustix::process::geteuid().as_raw();
    verify_bundle_for_owner(options, inspection, owner_uid)?;
    println!(
        "Verified exact current review bundle: {}",
        options.directory.display()
    );
    println!("No unit was installed, enabled, or started.");
    println!("Input remains unsupported until the remaining deployment gates pass.");
    Ok(())
}

fn verify_bundle_for_owner(
    options: &BundleOptions,
    inspection: &InspectedInputSet,
    owner_uid: u32,
) -> Result<RenderedBundle, String> {
    let expected = build_bundle(options, inspection)?;
    let directory_metadata = fs::symlink_metadata(&options.directory).map_err(|error| {
        format!(
            "cannot inspect bundle directory {}: {error}",
            options.directory.display()
        )
    })?;
    if !directory_metadata.file_type().is_dir()
        || directory_metadata.mode() & 0o777 != 0o700
        || directory_metadata.uid() != owner_uid
    {
        return Err("bundle directory must be a direct owner-bound mode-0700 directory".to_owned());
    }

    let expected_names = expected
        .files
        .iter()
        .map(|file| file.name.clone())
        .collect::<BTreeSet<_>>();
    let mut actual_names = BTreeSet::new();
    for entry in fs::read_dir(&options.directory)
        .map_err(|error| format!("cannot enumerate bundle directory: {error}"))?
    {
        if actual_names.len() >= expected.files.len() {
            return Err("bundle contains more files than the review format permits".to_owned());
        }
        let name = entry
            .map_err(|error| format!("cannot enumerate bundle directory: {error}"))?
            .file_name()
            .into_string()
            .map_err(|_| "bundle contains a non-UTF-8 file name".to_owned())?;
        actual_names.insert(name);
    }
    if actual_names != expected_names {
        return Err("bundle file set does not exactly match the current candidate".to_owned());
    }

    for expected_file in &expected.files {
        verify_bundle_file(
            &options.directory.join(&expected_file.name),
            &expected_file.contents,
            owner_uid,
        )?;
    }
    let directory_after = fs::symlink_metadata(&options.directory).map_err(|error| {
        format!(
            "cannot recheck bundle directory {}: {error}",
            options.directory.display()
        )
    })?;
    if directory_metadata.dev() != directory_after.dev()
        || directory_metadata.ino() != directory_after.ino()
        || !directory_after.file_type().is_dir()
        || directory_after.uid() != owner_uid
        || directory_after.mode() & 0o777 != 0o700
    {
        return Err("bundle directory changed during verification".to_owned());
    }
    Ok(expected)
}

fn verify_bundle_file(path: &Path, expected: &[u8], current_uid: u32) -> Result<(), String> {
    let before = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !before.file_type().is_file()
        || before.nlink() != 1
        || before.uid() != current_uid
        || before.mode() & 0o777 != 0o600
        || before.len() > MAX_BUNDLE_FILE_BYTES
        || before.len() != u64::try_from(expected.len()).unwrap_or(u64::MAX)
    {
        return Err(format!(
            "{} is not an exact private direct review file",
            path.display()
        ));
    }
    let actual =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let after = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot recheck {}: {error}", path.display()))?;
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || !after.file_type().is_file()
        || after.nlink() != 1
        || after.uid() != current_uid
        || after.mode() & 0o777 != 0o600
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || actual != expected
    {
        return Err(format!(
            "{} changed or differs from the current candidate",
            path.display()
        ));
    }
    Ok(())
}

fn install(options: BundleOptions) -> Result<(), String> {
    if rustix::process::geteuid().as_raw() != 0 {
        return Err("install requires an explicit root invocation".to_owned());
    }
    verify_packaged_executable(Path::new(BROKER_EXEC))?;
    verify_packaged_executable(Path::new(GUARD_EXEC))?;
    let inspection = inspect_devices(
        Path::new(SYSFS_INPUT),
        Path::new(DEV_INPUT),
        Path::new(UDEVADM),
        SUPPORTED_SEAT,
    )?;
    let bundle = verify_bundle_for_owner(&options, &inspection, options.uid)?;
    let enrollment_directory = Path::new(ENROLLMENT_BASE_DIRECTORY).join(options.uid.to_string());
    for directory in [
        Path::new("/etc/agent-seat"),
        Path::new(ENROLLMENT_BASE_DIRECTORY),
        enrollment_directory.as_path(),
    ] {
        ensure_install_directory(directory, true, 0)?;
    }
    ensure_install_directory(Path::new(SYSTEMD_UNIT_DIRECTORY), false, 0)?;
    install_rendered_bundle(
        &bundle,
        Path::new(SYSTEMD_UNIT_DIRECTORY),
        &enrollment_directory,
        None,
    )?;
    println!("Installed reviewed files for UID {}.", options.uid);
    println!("No service was enabled, reloaded, started, stopped, or rearmed.");
    println!("Provider policy and device permissions were not changed.");
    Ok(())
}

fn arm(options: ArmOptions) -> Result<(), String> {
    require_root("arm")?;
    verify_packaged_executable(Path::new(BROKER_EXEC))?;
    verify_packaged_executable(Path::new(GUARD_EXEC))?;
    let inspection = inspect_devices(
        Path::new(SYSFS_INPUT),
        Path::new(DEV_INPUT),
        Path::new(UDEVADM),
        SUPPORTED_SEAT,
    )?;
    let bundle_options = BundleOptions {
        uid: options.uid,
        session: options.session,
        directory: PathBuf::new(),
    };
    let bundle = build_bundle(&bundle_options, &inspection)?;
    let enrollment_directory = Path::new(ENROLLMENT_BASE_DIRECTORY).join(options.uid.to_string());
    ensure_install_directory(Path::new(SYSTEMD_UNIT_DIRECTORY), false, 0)?;
    ensure_install_directory(&enrollment_directory, false, 0)?;
    verify_installed_bundle(
        &bundle,
        Path::new(SYSTEMD_UNIT_DIRECTORY),
        &enrollment_directory,
        0,
    )?;

    let units = UnitNames::new(options.uid);
    run_systemctl(&["daemon-reload"])?;
    let _ = run_systemctl(&["reset-failed", &units.broker_service, &units.guard_service]);
    if let Err(error) = run_systemctl(&["start", &units.broker_service]) {
        let _ = stop_unit_names(&units);
        return Err(format!("cannot arm reviewed activity broker: {error}"));
    }
    if let Err(error) = require_units_active(&units) {
        let _ = stop_unit_names(&units);
        return Err(format!("armed units did not become active: {error}"));
    }
    thread::sleep(ARM_STABILITY_INTERVAL);
    if let Err(error) = require_units_active(&units) {
        let _ = stop_unit_names(&units);
        return Err(format!("armed units did not remain active: {error}"));
    }
    println!(
        "Armed one reviewed activity-broker cycle for UID {}.",
        options.uid
    );
    println!("No unit was enabled; any stop or evidence loss requires another explicit arm.");
    Ok(())
}

fn stop(uid: u32) -> Result<(), String> {
    require_root("stop")?;
    stop_unit_names(&UnitNames::new(uid))?;
    println!("Stopped the activity broker and eligibility guard for UID {uid}.");
    println!("Enrollment and provider policy were not changed.");
    Ok(())
}

fn purge(uid: u32) -> Result<(), String> {
    require_root("purge")?;
    let units = UnitNames::new(uid);
    run_systemctl(&["daemon-reload"])?;
    stop_unit_names(&units)?;
    let enrollment_directory = Path::new(ENROLLMENT_BASE_DIRECTORY).join(uid.to_string());
    ensure_install_directory(Path::new(SYSTEMD_UNIT_DIRECTORY), false, 0)?;
    ensure_install_directory(&enrollment_directory, false, 0)?;
    purge_installed_files(
        uid,
        Path::new(SYSTEMD_UNIT_DIRECTORY),
        &enrollment_directory,
        0,
    )?;
    run_systemctl(&["daemon-reload"])?;
    println!("Purged exact installed activity-broker files for UID {uid}.");
    println!("Provider policy, device permissions, groups, ACLs, and udev rules were not changed.");
    Ok(())
}

fn require_root(operation: &str) -> Result<(), String> {
    if rustix::process::geteuid().as_raw() == 0 {
        Ok(())
    } else {
        Err(format!("{operation} requires an explicit root invocation"))
    }
}

struct UnitNames {
    broker_service: String,
    broker_socket: String,
    guard_service: String,
    guard_socket: String,
}

impl UnitNames {
    fn new(uid: u32) -> Self {
        Self {
            broker_service: format!("agent-seat-activity-broker-{uid}.service"),
            broker_socket: format!("agent-seat-activity-broker-{uid}.socket"),
            guard_service: format!("agent-seat-eligibility-guard-{uid}.service"),
            guard_socket: format!("agent-seat-eligibility-guard-{uid}.socket"),
        }
    }
}

fn stop_unit_names(units: &UnitNames) -> Result<(), String> {
    run_systemctl(&[
        "stop",
        &units.broker_service,
        &units.broker_socket,
        &units.guard_service,
        &units.guard_socket,
    ])
}

fn require_units_active(units: &UnitNames) -> Result<(), String> {
    for unit in [
        &units.broker_service,
        &units.broker_socket,
        &units.guard_service,
        &units.guard_socket,
    ] {
        run_systemctl(&["is-active", "--quiet", unit])
            .map_err(|error| format!("{unit} is not active: {error}"))?;
    }
    Ok(())
}

fn run_systemctl(arguments: &[&str]) -> Result<(), String> {
    const TIMEOUT: Duration = Duration::from_secs(5);
    let mut child = Command::new("/usr/bin/systemctl")
        .args(arguments)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot execute systemctl: {error}"))?;
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("systemctl exited with {status}")),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                stop_child(&mut child);
                return Err("systemctl exceeded its five-second bound".to_owned());
            }
            Err(error) => {
                stop_child(&mut child);
                return Err(format!("cannot wait for systemctl: {error}"));
            }
        }
    }
}

fn verify_installed_bundle(
    bundle: &RenderedBundle,
    unit_directory: &Path,
    enrollment_directory: &Path,
    owner_uid: u32,
) -> Result<(), String> {
    let expected_enrollment_names = bundle
        .files
        .iter()
        .filter(|file| !(file.name.ends_with(".service") || file.name.ends_with(".socket")))
        .map(|file| file.name.as_str())
        .collect::<BTreeSet<_>>();
    let actual_enrollment_names = fs::read_dir(enrollment_directory)
        .map_err(|error| format!("cannot enumerate installed enrollment: {error}"))?
        .map(|entry| {
            entry
                .map_err(|error| format!("cannot enumerate installed enrollment: {error}"))?
                .file_name()
                .into_string()
                .map_err(|_| "installed enrollment contains a non-UTF-8 name".to_owned())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actual_enrollment_names
        != expected_enrollment_names
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    {
        return Err("installed enrollment file set is not exact".to_owned());
    }
    for file in &bundle.files {
        let directory = if file.name.ends_with(".service") || file.name.ends_with(".socket") {
            unit_directory
        } else {
            enrollment_directory
        };
        verify_bundle_file(&directory.join(&file.name), &file.contents, owner_uid)?;
    }
    Ok(())
}

fn purge_installed_files(
    uid: u32,
    unit_directory: &Path,
    enrollment_directory: &Path,
    owner_uid: u32,
) -> Result<(), String> {
    let units = UnitNames::new(uid);
    let unit_names = [
        units.broker_service,
        units.broker_socket,
        units.guard_service,
        units.guard_socket,
    ];
    let enrollment_names = [
        "initial-input-set.v1",
        "enrolled-device-set.v1",
        "REVIEW.txt",
    ];
    let actual = fs::read_dir(enrollment_directory)
        .map_err(|error| format!("cannot enumerate installed enrollment: {error}"))?
        .map(|entry| {
            entry
                .map_err(|error| format!("cannot enumerate installed enrollment: {error}"))?
                .file_name()
                .into_string()
                .map_err(|_| "installed enrollment contains a non-UTF-8 name".to_owned())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actual != enrollment_names.into_iter().map(str::to_owned).collect() {
        return Err("installed enrollment contains an unexpected file; refusing purge".to_owned());
    }
    for name in &unit_names {
        verify_removable_file(&unit_directory.join(name), owner_uid)?;
    }
    for name in enrollment_names {
        verify_removable_file(&enrollment_directory.join(name), owner_uid)?;
    }
    for name in enrollment_names {
        fs::remove_file(enrollment_directory.join(name))
            .map_err(|error| format!("cannot remove enrollment file {name}: {error}"))?;
    }
    for name in unit_names {
        fs::remove_file(unit_directory.join(&name))
            .map_err(|error| format!("cannot remove unit file {name}: {error}"))?;
    }
    fs::remove_dir(enrollment_directory).map_err(|error| {
        format!(
            "cannot remove empty enrollment directory {}: {error}",
            enrollment_directory.display()
        )
    })?;
    fs::File::open(unit_directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot synchronize unit directory: {error}"))?;
    Ok(())
}

fn verify_removable_file(path: &Path, owner_uid: u32) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect purge target {}: {error}", path.display()))?;
    if !metadata.file_type().is_file()
        || metadata.nlink() != 1
        || metadata.uid() != owner_uid
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(format!(
            "{} is not an exact private installed file; refusing purge",
            path.display()
        ));
    }
    Ok(())
}

fn verify_packaged_executable(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "cannot inspect packaged executable {}: {error}",
            path.display()
        )
    })?;
    if !metadata.file_type().is_file()
        || metadata.nlink() != 1
        || metadata.uid() != 0
        || metadata.mode() & 0o111 == 0
        || metadata.mode() & 0o022 != 0
        || metadata.len() == 0
    {
        return Err(format!(
            "{} is not an exact root-owned packaged executable",
            path.display()
        ));
    }
    Ok(())
}

fn ensure_install_directory(path: &Path, create: bool, owner_uid: u32) -> Result<(), String> {
    if create {
        match fs::create_dir(path) {
            Ok(()) => {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
                    format!(
                        "cannot make install directory private {}: {error}",
                        path.display()
                    )
                })?;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("cannot prepare {}: {error}", path.display())),
        }
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.file_type().is_dir() || metadata.uid() != owner_uid || metadata.mode() & 0o022 != 0
    {
        return Err(format!(
            "{} is not a direct owner-controlled install directory",
            path.display()
        ));
    }
    Ok(())
}

struct InstallEntry {
    directory: usize,
    temporary: String,
    target: String,
    published: bool,
}

struct InstallTransaction {
    directories: [fs::File; 2],
    entries: Vec<InstallEntry>,
    finished: bool,
}

impl InstallTransaction {
    fn new(unit_directory: &Path, enrollment_directory: &Path) -> Result<Self, String> {
        let units = fs::File::open(unit_directory)
            .map_err(|error| format!("cannot open {}: {error}", unit_directory.display()))?;
        let enrollment = fs::File::open(enrollment_directory)
            .map_err(|error| format!("cannot open {}: {error}", enrollment_directory.display()))?;
        Ok(Self {
            directories: [units, enrollment],
            entries: Vec::with_capacity(7),
            finished: false,
        })
    }

    fn stage(&mut self, directory: usize, index: usize, file: &RenderedFile) -> Result<(), String> {
        let temporary = format!(".agent-seat-install-{}-{index}", std::process::id());
        let descriptor = openat(
            &self.directories[directory],
            &temporary,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|error| format!("cannot stage {}: {error}", file.name))?;
        let mut output = fs::File::from(descriptor);
        self.entries.push(InstallEntry {
            directory,
            temporary,
            target: file.name.clone(),
            published: false,
        });
        output
            .set_permissions(fs::Permissions::from_mode(0o600))
            .and_then(|()| output.write_all(&file.contents))
            .and_then(|()| output.sync_all())
            .map_err(|error| format!("cannot write staged {}: {error}", file.name))
    }

    fn publish(&mut self, fail_after: Option<usize>) -> Result<(), String> {
        for (index, entry) in self.entries.iter_mut().enumerate() {
            if fail_after == Some(index) {
                return Err("injected install publication failure".to_owned());
            }
            renameat_with(
                &self.directories[entry.directory],
                &entry.temporary,
                &self.directories[entry.directory],
                &entry.target,
                RenameFlags::NOREPLACE,
            )
            .map_err(|error| format!("cannot publish {}: {error}", entry.target))?;
            entry.published = true;
        }
        for directory in &self.directories {
            directory
                .sync_all()
                .map_err(|error| format!("cannot synchronize install directory: {error}"))?;
        }
        self.finished = true;
        Ok(())
    }
}

impl Drop for InstallTransaction {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        for entry in self.entries.iter().rev() {
            let name = if entry.published {
                &entry.target
            } else {
                &entry.temporary
            };
            let _ = unlinkat(&self.directories[entry.directory], name, AtFlags::empty());
        }
    }
}

fn install_rendered_bundle(
    bundle: &RenderedBundle,
    unit_directory: &Path,
    enrollment_directory: &Path,
    fail_after: Option<usize>,
) -> Result<(), String> {
    let mut transaction = InstallTransaction::new(unit_directory, enrollment_directory)?;
    for (index, file) in bundle.files.iter().enumerate() {
        let directory =
            usize::from(!(file.name.ends_with(".service") || file.name.ends_with(".socket")));
        let target = [unit_directory, enrollment_directory][directory].join(&file.name);
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                return Err(format!(
                    "install target already exists: {}",
                    target.display()
                ));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect {}: {error}", target.display())),
        }
        transaction.stage(directory, index, file)?;
    }
    transaction.publish(fail_after)
}

struct NewBundleDirectory {
    path: PathBuf,
    files: Vec<PathBuf>,
    finished: bool,
}

impl NewBundleDirectory {
    fn create(path: &Path) -> Result<Self, String> {
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700).create(path).map_err(|error| {
            format!(
                "cannot create new review directory {}: {error}",
                path.display()
            )
        })?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            let _ = fs::remove_dir(path);
            format!(
                "cannot make review directory private {}: {error}",
                path.display()
            )
        })?;
        Ok(Self {
            path: path.to_owned(),
            files: Vec::with_capacity(7),
            finished: false,
        })
    }

    fn write(&mut self, name: &str, source: &[u8]) -> Result<(), String> {
        let path = self.path.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        self.files.push(path.clone());
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("cannot make {} private: {error}", path.display()))?;
        file.write_all(source)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("cannot write {}: {error}", path.display()))
    }

    fn finish(&mut self) {
        self.finished = true;
    }
}

impl Drop for NewBundleDirectory {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        for path in self.files.iter().rev() {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_dir(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_seat_activity_broker::{decode_device_set, decode_input_set};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn options(values: &[&str]) -> Result<Options, String> {
        Options::parse(values.iter().map(OsString::from))
    }

    fn inspection(devices: Vec<InspectedDevice>) -> InspectedInputSet {
        let mappings = devices
            .iter()
            .map(|device| {
                InputClassMapping::new(device.event_number, device.sysfs_path.clone())
                    .expect("valid input-set fixture")
            })
            .collect();
        InspectedInputSet { mappings, devices }
    }

    fn identity(serial: Option<&str>) -> DeviceIdentity {
        DeviceIdentity::new(
            "pci-0000:00:01.0-usb-0:1:1.0".to_owned(),
            Some("usb".to_owned()),
            Some("046d".to_owned()),
            Some("c548".to_owned()),
            Some("0504".to_owned()),
            serial.map(str::to_owned),
        )
        .expect("identity fixture")
    }

    fn capabilities() -> DeviceCapabilities {
        capabilities_with_event_types("17")
    }

    fn capabilities_with_event_types(event_types: &str) -> DeviceCapabilities {
        DeviceCapabilities::new([
            "0".to_owned(),
            event_types.to_owned(),
            "0".to_owned(),
            "ffff0000 0".to_owned(),
            "0".to_owned(),
            "10".to_owned(),
            "1943".to_owned(),
            "0".to_owned(),
            "0".to_owned(),
            "0".to_owned(),
        ])
        .expect("capability fixture")
    }

    #[test]
    fn cli_requires_one_explicit_bounded_command() {
        assert_eq!(options(&["enroll", "--help"]), Ok(Options::Help));
        assert_eq!(options(&["enroll", "inspect"]), Ok(Options::Inspect));
        assert_eq!(
            options(&["enroll", "inspect", "--seat", "seat0"]),
            Ok(Options::Inspect)
        );
        assert!(options(&["enroll"]).is_err());
        assert!(options(&["enroll", "inspect", "--seat", "seat1"]).is_err());
        assert!(options(&["enroll", "inspect", "--seat", "seat0", "extra"]).is_err());
        assert!(options(&["enroll", "--help", "extra"]).is_err());

        let rendered = options(&[
            "enroll",
            "render",
            "--uid",
            "1000",
            "--session",
            "68",
            "--output",
            "/tmp/review",
        ]);
        assert_eq!(
            rendered,
            Ok(Options::Render(BundleOptions {
                uid: 1000,
                session: "68".to_owned(),
                directory: PathBuf::from("/tmp/review"),
            }))
        );
        assert_eq!(
            options(&[
                "enroll",
                "verify",
                "--uid",
                "1000",
                "--session",
                "68",
                "--bundle",
                "/tmp/review",
            ]),
            Ok(Options::Verify(BundleOptions {
                uid: 1000,
                session: "68".to_owned(),
                directory: PathBuf::from("/tmp/review"),
            }))
        );
        assert_eq!(
            options(&[
                "enroll",
                "install",
                "--uid",
                "1000",
                "--session",
                "68",
                "--bundle",
                "/tmp/review",
                "--confirm-install",
            ]),
            Ok(Options::Install(BundleOptions {
                uid: 1000,
                session: "68".to_owned(),
                directory: PathBuf::from("/tmp/review"),
            }))
        );
        assert!(
            options(&[
                "enroll",
                "install",
                "--uid",
                "1000",
                "--session",
                "68",
                "--bundle",
                "/tmp/review",
            ])
            .is_err()
        );
        assert_eq!(
            options(&[
                "enroll",
                "arm",
                "--uid",
                "1000",
                "--session",
                "68",
                "--confirm-arm",
            ]),
            Ok(Options::Arm(ArmOptions {
                uid: 1000,
                session: "68".to_owned(),
            }))
        );
        assert_eq!(
            options(&["enroll", "stop", "--uid", "1000"]),
            Ok(Options::Stop(1000))
        );
        assert_eq!(
            options(&["enroll", "purge", "--uid", "1000", "--confirm-purge",]),
            Ok(Options::Purge(1000))
        );
        assert!(options(&["enroll", "purge", "--uid", "1000"]).is_err());
        assert!(options(&["enroll", "arm", "--uid", "1000", "--session", "68"]).is_err());
        assert!(options(&["enroll", "render", "--uid", "0"]).is_err());
        assert!(
            options(&[
                "enroll",
                "render",
                "--uid",
                "1000",
                "--session",
                "../68",
                "--output",
                "/tmp/review",
            ])
            .is_err()
        );
        assert!(
            options(&[
                "enroll",
                "render",
                "--uid",
                "1000",
                "--session",
                "68",
                "--output",
                "relative",
            ])
            .is_err()
        );
    }

    #[test]
    fn event_names_are_canonical_and_numeric() {
        assert_eq!(event_number(OsStr::new("event0")), Some(0));
        assert_eq!(event_number(OsStr::new("event42")), Some(42));
        assert_eq!(event_number(OsStr::new("event")), None);
        assert_eq!(event_number(OsStr::new("event-1")), None);
        assert_eq!(event_number(OsStr::new("mouse0")), None);

        let mut entries = vec![
            (10, PathBuf::from("event10")),
            (2, PathBuf::from("event2")),
            (1, PathBuf::from("event1")),
        ];
        sort_event_entries(&mut entries);
        assert_eq!(
            entries
                .iter()
                .map(|(number, _)| *number)
                .collect::<Vec<_>>(),
            [1, 2, 10]
        );
    }

    #[test]
    fn relevant_properties_are_conservative_and_strict() {
        let properties =
            Properties::parse(b"ID_INPUT=1\nID_INPUT_KEY=1\nID_INPUT_KEYBOARD=1\nID_SEAT=seat0\n")
                .expect("valid udev fixture");
        assert_eq!(properties.seat(), Ok("seat0"));
        assert_eq!(properties.relevant_classes(), Ok(vec!["key", "keyboard"]));

        let non_input = Properties::parse(b"ID_INPUT_KEY=1\n").expect("non-input fixture");
        assert!(
            non_input
                .relevant_classes()
                .expect("classification")
                .is_empty()
        );
        assert_eq!(non_input.seat(), Ok("seat0"));

        let identified = Properties::parse(
            b"ID_INPUT=1\nID_PATH=pci-0000:00:01.0-usb-0:1:1.0\nID_BUS=usb\nID_VENDOR_ID=046d\nID_MODEL_ID=c548\nID_SERIAL_SHORT=receiver-1\n",
        )
        .expect("identity properties");
        assert_eq!(
            identified.identity().expect("device identity").strength(),
            IdentityStrength::Serial
        );
        assert!(properties.identity().is_err());

        assert!(Properties::parse(b"ID_INPUT=1\nID_INPUT=1\n").is_err());
        assert!(Properties::parse(b"NAME=keyboard\n").is_err());
        assert!(
            Properties::parse(b"ID_INPUT=1\nID_INPUT_MOUSE=yes\n")
                .expect("parse invalid flag fixture")
                .relevant_classes()
                .is_err()
        );
        assert!(
            Properties::parse(b"ID_INPUT=1\nID_INPUT_MOUSE=1\nID_SEAT=../seat0\n")
                .expect("parse invalid seat fixture")
                .seat()
                .is_err()
        );
    }

    #[test]
    fn sysfs_paths_are_single_absolute_device_paths() {
        assert_eq!(
            parse_sysfs_path(b"/devices/platform/input/input0/event0\n"),
            Ok(PathBuf::from("/sys/devices/platform/input/input0/event0"))
        );
        for malformed in [
            &b"devices/input0\n"[..],
            &b"/devices/../input0\n"[..],
            &b"/devices/input0\nextra\n"[..],
            &b""[..],
        ] {
            assert!(parse_sysfs_path(malformed).is_err());
        }
    }

    #[test]
    fn report_contains_only_reviewable_coarse_metadata() {
        let devices = [InspectedDevice {
            event_number: 2,
            device_node: PathBuf::from("/dev/input/event2"),
            sysfs_path: PathBuf::from("/sys/devices/example/input/input2/event2"),
            classes: vec!["mouse"],
            identity: identity(None),
            capabilities: capabilities(),
            device_id: 13,
        }];
        let mut report = Vec::new();
        write_report(&mut report, SUPPORTED_SEAT, &devices).expect("render report fixture");
        let report = String::from_utf8(report).expect("report is UTF-8");
        assert!(report.contains("device=/dev/input/event2"));
        assert!(report.contains("classes=mouse"));
        assert!(report.contains("review_only=true"));
        assert!(!report.contains("device_id"));
    }

    #[test]
    fn metadata_command_output_is_capped_while_reading() {
        assert_eq!(
            run_udevadm(Path::new("/usr/bin/printf"), [OsStr::new("bounded")]),
            Ok(b"bounded".to_vec())
        );
        let oversized = "x".repeat(MAX_UDEV_OUTPUT_BYTES + 1);
        assert!(run_udevadm(Path::new("/usr/bin/printf"), [OsStr::new(&oversized)]).is_err());
    }

    #[test]
    fn render_writes_a_private_inert_exact_set_without_overwrite() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let output = std::env::temp_dir().join(format!(
            "agent-seat-render-test-{}-{nonce}",
            std::process::id()
        ));
        let options = BundleOptions {
            uid: 1000,
            session: "68".to_owned(),
            directory: output.clone(),
        };
        let mut inspection = inspection(vec![InspectedDevice {
            event_number: 2,
            device_node: PathBuf::from("/dev/input/event2"),
            sysfs_path: PathBuf::from("/sys/devices/example/input/input2/event2"),
            classes: vec!["mouse"],
            identity: identity(None),
            capabilities: capabilities(),
            device_id: 13,
        }]);
        inspection.mappings.push(
            InputClassMapping::new(
                10,
                PathBuf::from("/sys/devices/example/input/input10/event10"),
            )
            .expect("irrelevant input-class fixture"),
        );

        render_bundle(&options, &inspection).expect("render fixture");
        assert_eq!(
            fs::metadata(&output)
                .expect("bundle metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let expected = [
            "agent-seat-activity-broker-1000.service",
            "agent-seat-activity-broker-1000.socket",
            "agent-seat-eligibility-guard-1000.service",
            "agent-seat-eligibility-guard-1000.socket",
            "initial-input-set.v1",
            "enrolled-device-set.v1",
            "REVIEW.txt",
        ];
        for name in expected {
            let path = output.join(name);
            assert!(path.is_file(), "missing rendered file {name}");
            assert_eq!(
                fs::metadata(path)
                    .expect("file metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let service = fs::read_to_string(output.join(expected[0])).expect("rendered service");
        assert!(service.contains("OpenFile=/dev/input/event2:event0:read-only"));
        assert!(service.contains("ExecStart=/usr/bin/agent-seat-activity-broker --uid 1000"));
        assert!(service.contains(
            "OpenFile=/etc/agent-seat/activity/1000/enrolled-device-set.v1:enrolled-device-set:read-only"
        ));
        assert!(service.contains("StandardInput=file:/run/agent-seat/1000/eligibility.sock"));
        assert!(service.contains("StandardOutput=socket"));
        let guard_service =
            fs::read_to_string(output.join(expected[2])).expect("rendered guard service");
        assert!(!guard_service.contains("--input-set-fd"));
        assert!(guard_service.contains(
            "OpenFile=/etc/agent-seat/activity/1000/initial-input-set.v1:initial-input-set:read-only"
        ));
        let initial_set = fs::read(output.join("initial-input-set.v1")).expect("input set");
        assert_eq!(
            decode_input_set(&initial_set),
            Ok(inspection.mappings.clone())
        );
        let device_set = fs::read(output.join("enrolled-device-set.v1")).expect("device set");
        assert_eq!(
            decode_device_set(&device_set)
                .expect("decode device set")
                .len(),
            1
        );
        let review = fs::read_to_string(output.join("REVIEW.txt")).expect("review record");
        assert!(review.contains("review_only=true"));
        assert!(review.contains("supported=false"));
        assert!(review.contains("device=/dev/input/event2"));
        assert!(review.contains("complete_input_class_count=2"));
        assert!(review.contains("identity_strength=topology"));
        assert!(review.contains("coverage_evidence=topology+capabilities"));
        verify_bundle(&options, &inspection).expect("verify exact fixture");
        inspection.devices[0].identity = identity(Some("replacement"));
        assert!(verify_bundle(&options, &inspection).is_err());
        inspection.devices[0].identity = identity(None);
        inspection.devices[0].capabilities = capabilities_with_event_types("1f");
        assert!(verify_bundle(&options, &inspection).is_err());
        inspection.devices[0].capabilities = capabilities();
        fs::set_permissions(output.join("REVIEW.txt"), fs::Permissions::from_mode(0o644))
            .expect("weaken fixture permissions");
        assert!(verify_bundle(&options, &inspection).is_err());
        assert!(render_bundle(&options, &inspection).is_err());

        fs::remove_dir_all(output).expect("remove exact test fixture");
    }

    #[test]
    fn install_transaction_is_new_only_private_and_rolls_back_partial_publication() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "agent-seat-install-test-{}-{nonce}",
            std::process::id()
        ));
        let units = root.join("units");
        let enrollment = root.join("enrollment");
        let rollback_units = root.join("rollback-units");
        let rollback_enrollment = root.join("rollback-enrollment");
        for directory in [
            &root,
            &units,
            &enrollment,
            &rollback_units,
            &rollback_enrollment,
        ] {
            fs::create_dir(directory).expect("install fixture directory");
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
                .expect("private install fixture directory");
        }
        let options = BundleOptions {
            uid: 1000,
            session: "68".to_owned(),
            directory: root.join("unused-review"),
        };
        let inspection = inspection(vec![InspectedDevice {
            event_number: 2,
            device_node: PathBuf::from("/dev/input/event2"),
            sysfs_path: PathBuf::from("/sys/devices/example/input/input2/event2"),
            classes: vec!["mouse"],
            identity: identity(None),
            capabilities: capabilities(),
            device_id: 13,
        }]);
        let bundle = build_bundle(&options, &inspection).expect("install bundle fixture");
        let owner_uid = rustix::process::geteuid().as_raw();
        ensure_install_directory(&units, false, owner_uid).expect("checked unit directory");
        ensure_install_directory(&enrollment, false, owner_uid)
            .expect("checked enrollment directory");

        install_rendered_bundle(&bundle, &units, &enrollment, None).expect("install exact fixture");
        verify_installed_bundle(&bundle, &units, &enrollment, owner_uid)
            .expect("verify installed fixture");
        assert_eq!(fs::read_dir(&units).expect("installed units").count(), 4);
        assert_eq!(
            fs::read_dir(&enrollment)
                .expect("installed enrollment")
                .count(),
            3
        );
        for file in &bundle.files {
            let directory = if file.name.ends_with(".service") || file.name.ends_with(".socket") {
                &units
            } else {
                &enrollment
            };
            let installed = directory.join(&file.name);
            assert_eq!(
                fs::read(&installed).expect("installed contents"),
                file.contents
            );
            assert_eq!(
                fs::metadata(installed)
                    .expect("installed metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(install_rendered_bundle(&bundle, &units, &enrollment, None).is_err());
        fs::write(enrollment.join("unexpected"), b"unexpected").expect("extra enrollment file");
        assert!(verify_installed_bundle(&bundle, &units, &enrollment, owner_uid).is_err());
        fs::remove_file(enrollment.join("unexpected")).expect("remove extra enrollment file");

        assert!(
            install_rendered_bundle(&bundle, &rollback_units, &rollback_enrollment, Some(3),)
                .is_err()
        );
        assert_eq!(
            fs::read_dir(&rollback_units)
                .expect("rollback units")
                .count(),
            0
        );
        assert_eq!(
            fs::read_dir(&rollback_enrollment)
                .expect("rollback enrollment")
                .count(),
            0
        );

        purge_installed_files(1000, &units, &enrollment, owner_uid)
            .expect("purge exact installed fixture");
        assert_eq!(fs::read_dir(&units).expect("purged units").count(), 0);
        assert!(!enrollment.exists());

        fs::remove_dir_all(root).expect("remove install fixture");
    }
}
