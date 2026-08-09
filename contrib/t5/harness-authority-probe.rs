//! Dependency-free hostile probe for a participating harness launcher.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File, OpenOptions};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_PATH_BYTES: usize = 4096;
const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
const TRUE: &str = "/usr/bin/true";
const MANAGER_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(2);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy)]
enum Expectation {
    Baseline,
    Confined,
}

impl Expectation {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "baseline" => Ok(Self::Baseline),
            "confined" => Ok(Self::Confined),
            _ => Err("--expect must be baseline or confined".to_owned()),
        }
    }
}

struct Options {
    expectation: Expectation,
    input_device: PathBuf,
    uinput_device: PathBuf,
    xauthority: PathBuf,
    provider_socket: PathBuf,
    broker_socket: PathBuf,
    manager_socket: PathBuf,
    parent_pid: u32,
    display: u16,
}

impl Options {
    fn parse() -> Result<Option<Self>, String> {
        let mut arguments = env::args();
        let _program = arguments.next();
        let arguments: Vec<String> = arguments.collect();
        if arguments.as_slice() == ["--help"] {
            return Ok(None);
        }
        if arguments.len() != 18 {
            return Err("exactly nine option/value pairs are required; use --help".to_owned());
        }

        let mut expectation = None;
        let mut input_device = None;
        let mut uinput_device = None;
        let mut xauthority = None;
        let mut provider_socket = None;
        let mut broker_socket = None;
        let mut manager_socket = None;
        let mut parent_pid = None;
        let mut display = None;

        for pair in arguments.chunks_exact(2) {
            let value = pair[1].as_str();
            match pair[0].as_str() {
                "--expect" => set_once(&mut expectation, Expectation::parse(value)?)?,
                "--input-device" => set_once(&mut input_device, absolute_path(value)?)?,
                "--uinput-device" => set_once(&mut uinput_device, absolute_path(value)?)?,
                "--xauthority" => set_once(&mut xauthority, absolute_path(value)?)?,
                "--provider-socket" => {
                    set_once(&mut provider_socket, absolute_path(value)?)?;
                }
                "--broker-socket" => set_once(&mut broker_socket, absolute_path(value)?)?,
                "--manager-socket" => set_once(&mut manager_socket, absolute_path(value)?)?,
                "--parent-pid" => {
                    let parsed = value
                        .parse::<u32>()
                        .map_err(|_| "--parent-pid must be a nonzero decimal u32".to_owned())?;
                    if parsed == 0 {
                        return Err("--parent-pid must be a nonzero decimal u32".to_owned());
                    }
                    set_once(&mut parent_pid, parsed)?;
                }
                "--display" => {
                    let parsed = value
                        .parse::<u16>()
                        .map_err(|_| "--display must be a decimal u16".to_owned())?;
                    set_once(&mut display, parsed)?;
                }
                _ => return Err(format!("unknown option {}; use --help", pair[0])),
            }
        }

        Ok(Some(Self {
            expectation: expectation.ok_or("missing --expect")?,
            input_device: input_device.ok_or("missing --input-device")?,
            uinput_device: uinput_device.ok_or("missing --uinput-device")?,
            xauthority: xauthority.ok_or("missing --xauthority")?,
            provider_socket: provider_socket.ok_or("missing --provider-socket")?,
            broker_socket: broker_socket.ok_or("missing --broker-socket")?,
            manager_socket: manager_socket.ok_or("missing --manager-socket")?,
            parent_pid: parent_pid.ok_or("missing --parent-pid")?,
            display: display.ok_or("missing --display")?,
        }))
    }
}

struct Reachability {
    input_device: bool,
    uinput_device: bool,
    xauthority: bool,
    provider_socket: bool,
    broker_socket: bool,
    manager_socket: bool,
    manager_submit: bool,
    x11_filesystem: bool,
    x11_abstract: bool,
    parent_process: bool,
    inherited_input: bool,
}

impl Reachability {
    fn inspect(options: &Options) -> Result<Self, String> {
        let display_name = format!("/tmp/.X11-unix/X{}", options.display);
        let x11_filesystem_path = Path::new(&display_name);
        let x11_abstract_address = SocketAddr::from_abstract_name(display_name.as_bytes())
            .map_err(|error| format!("cannot construct bounded X11 abstract address: {error}"))?;
        // Sample first: a successful probe open must not be mistaken for an
        // inherited descriptor through temporary-lifetime extension.
        let inherited_input = inherited_input_descriptor()?;

        Ok(Self {
            input_device: File::open(&options.input_device).is_ok(),
            uinput_device: OpenOptions::new()
                .read(true)
                .write(true)
                .open(&options.uinput_device)
                .is_ok(),
            xauthority: File::open(&options.xauthority).is_ok(),
            provider_socket: UnixStream::connect(&options.provider_socket).is_ok(),
            broker_socket: UnixStream::connect(&options.broker_socket).is_ok(),
            manager_socket: UnixStream::connect(&options.manager_socket).is_ok(),
            manager_submit: manager_submit()?,
            x11_filesystem: UnixStream::connect(x11_filesystem_path).is_ok(),
            x11_abstract: UnixStream::connect_addr(&x11_abstract_address).is_ok(),
            parent_process: File::open(format!("/proc/{}/status", options.parent_pid)).is_ok(),
            inherited_input,
        })
    }

    fn verdict(&self, expectation: Expectation) -> bool {
        let directly_reachable = [
            self.input_device,
            self.uinput_device,
            self.xauthority,
            self.provider_socket,
            self.broker_socket,
            self.manager_socket,
            self.manager_submit,
            self.x11_filesystem,
            self.x11_abstract,
            self.parent_process,
        ];
        match expectation {
            Expectation::Baseline => directly_reachable.into_iter().all(|value| value),
            Expectation::Confined => {
                directly_reachable.into_iter().all(|value| !value) && !self.inherited_input
            }
        }
    }

    fn print(&self, passed: bool) {
        println!(
            concat!(
                "{{\"format\":\"agent-seat.t5-harness-probe/1\",",
                "\"input_device\":{},\"uinput_device\":{},",
                "\"xauthority\":{},\"provider_socket\":{},",
                "\"broker_socket\":{},\"manager_socket\":{},",
                "\"manager_submit\":{},\"x11_filesystem\":{},",
                "\"x11_abstract\":{},\"parent_process\":{},",
                "\"inherited_input\":{},\"passed\":{}}}"
            ),
            self.input_device,
            self.uinput_device,
            self.xauthority,
            self.provider_socket,
            self.broker_socket,
            self.manager_socket,
            self.manager_submit,
            self.x11_filesystem,
            self.x11_abstract,
            self.parent_process,
            self.inherited_input,
            passed,
        );
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<(), String> {
    if slot.is_some() {
        return Err("duplicate option".to_owned());
    }
    *slot = Some(value);
    Ok(())
}

fn absolute_path(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if value.is_empty() || value.as_bytes().len() > MAX_PATH_BYTES || !path.is_absolute() {
        return Err("paths must be nonempty absolute values of at most 4096 bytes".to_owned());
    }
    Ok(path)
}

fn manager_submit() -> Result<bool, String> {
    let child = Command::new(SYSTEMD_RUN)
        .args([
            "--user",
            "--wait",
            "--collect",
            "--quiet",
            "--service-type=exec",
            "--",
            TRUE,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return Ok(false);
    };
    let deadline = Instant::now() + MANAGER_ATTEMPT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) if Instant::now() < deadline => thread::sleep(CHILD_POLL_INTERVAL),
            Ok(None) => {
                child
                    .kill()
                    .map_err(|error| format!("cannot stop bounded manager attempt: {error}"))?;
                child
                    .wait()
                    .map_err(|error| format!("cannot reap bounded manager attempt: {error}"))?;
                return Ok(false);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("cannot observe bounded manager attempt: {error}"));
            }
        }
    }
}

fn inherited_input_descriptor() -> Result<bool, String> {
    let descriptors = fs::read_dir("/proc/self/fd")
        .map_err(|error| format!("cannot enumerate inherited descriptors: {error}"))?;
    for descriptor in descriptors {
        let descriptor = descriptor
            .map_err(|error| format!("cannot enumerate inherited descriptor: {error}"))?;
        let target = fs::read_link(descriptor.path())
            .map_err(|error| format!("cannot inspect inherited descriptor: {error}"))?;
        let bytes = target.as_os_str().as_bytes();
        if bytes == b"/dev/uinput" || bytes.starts_with(b"/dev/input/") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn print_help() {
    println!(
        "Usage: harness-authority-probe \
--expect baseline|confined \
--input-device ABSOLUTE_EVENT_PATH \
--uinput-device ABSOLUTE_UINPUT_PATH \
--xauthority ABSOLUTE_XAUTHORITY_PATH \
--provider-socket ABSOLUTE_PROVIDER_SOCKET \
--broker-socket ABSOLUTE_BROKER_SOCKET \
--manager-socket ABSOLUTE_USER_MANAGER_SOCKET \
--parent-pid NONZERO_PID \
--display DISPLAY_NUMBER"
    );
}

fn run() -> Result<ExitCode, String> {
    let Some(options) = Options::parse()? else {
        print_help();
        return Ok(ExitCode::SUCCESS);
    };
    let reachability = Reachability::inspect(&options)?;
    let passed = reachability.verdict(options.expectation);
    reachability.print(passed);
    Ok(if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("harness-authority-probe: {error}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expectation_parser_is_closed() {
        assert!(matches!(
            Expectation::parse("baseline"),
            Ok(Expectation::Baseline)
        ));
        assert!(matches!(
            Expectation::parse("confined"),
            Ok(Expectation::Confined)
        ));
        assert!(Expectation::parse("other").is_err());
    }

    #[test]
    fn paths_are_absolute_nonempty_and_bounded() {
        assert_eq!(
            absolute_path("/run/probe").expect("valid probe path"),
            PathBuf::from("/run/probe")
        );
        assert!(absolute_path("").is_err());
        assert!(absolute_path("relative").is_err());
        let over_bound = format!("/{}", "a".repeat(MAX_PATH_BYTES));
        assert!(absolute_path(&over_bound).is_err());
    }

    #[test]
    fn duplicate_values_are_rejected() {
        let mut slot = None;
        set_once(&mut slot, 1_u8).expect("first value");
        assert!(set_once(&mut slot, 2_u8).is_err());
        assert_eq!(slot, Some(1));
    }

    #[test]
    fn verdict_requires_meaningful_baseline_and_complete_denial() {
        let reachable = reachability(true, false);
        assert!(reachable.verdict(Expectation::Baseline));
        assert!(!reachable.verdict(Expectation::Confined));

        let denied = reachability(false, false);
        assert!(!denied.verdict(Expectation::Baseline));
        assert!(denied.verdict(Expectation::Confined));

        let inherited = reachability(false, true);
        assert!(!inherited.verdict(Expectation::Confined));
    }

    fn reachability(value: bool, inherited_input: bool) -> Reachability {
        Reachability {
            input_device: value,
            uinput_device: value,
            xauthority: value,
            provider_socket: value,
            broker_socket: value,
            manager_socket: value,
            manager_submit: value,
            x11_filesystem: value,
            x11_abstract: value,
            parent_process: value,
            inherited_input,
        }
    }
}
