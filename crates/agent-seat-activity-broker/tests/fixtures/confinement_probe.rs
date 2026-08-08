//! Single-process hostile probe for the systemd confinement profile.

#![forbid(unsafe_code)]

use std::env;
use std::fs::File;
use std::io::Read as _;
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt as _;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match env::args().nth(1).as_deref() {
        Some(profile @ ("broker" | "guard")) => finish(probe(profile)),
        _ => finish(Err("expected broker or guard profile".to_owned())),
    }
}

fn finish(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => {
            println!("agent-seat-confinement-probe=pass");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("agent-seat-confinement-probe: {error}");
            ExitCode::FAILURE
        }
    }
}

fn probe(profile: &str) -> Result<(), String> {
    let inherited = read_to_string("/proc/self/fd/3")?;
    if inherited != "inherited-evidence\n" {
        return Err("the exact inherited read-only descriptor changed".to_owned());
    }
    let fdinfo = read_to_string("/proc/self/fdinfo/3")?;
    let flags = fdinfo
        .lines()
        .find_map(|line| line.strip_prefix("flags:\t"))
        .ok_or_else(|| "the inherited descriptor has no kernel flags".to_owned())?;
    let flags = u32::from_str_radix(flags, 8)
        .map_err(|_| "the inherited descriptor flags are malformed".to_owned())?;
    if flags & 0b11 != 0 {
        return Err("the inherited descriptor was not opened read-only".to_owned());
    }

    for (label, path) in [
        ("home", required("AGENT_SEAT_HOME_SECRET")?),
        ("runtime", required("AGENT_SEAT_RUNTIME_SECRET")?),
        ("another process", required("AGENT_SEAT_PARENT_ENVIRON")?),
        ("input event", "/dev/input/event0".to_owned()),
    ] {
        if File::open(path).is_ok() {
            return Err(format!("{label} metadata remained readable"));
        }
    }

    if UnixStream::connect(required("AGENT_SEAT_HOST_SOCKET")?).is_ok() {
        return Err("a host AF_UNIX socket remained reachable".to_owned());
    }
    if profile == "broker" {
        if UnixListener::bind(required("AGENT_SEAT_CHILD_SOCKET")?).is_ok() {
            return Err("the broker created an AF_UNIX socket".to_owned());
        }
    } else if UnixStream::connect("/run/dbus/system_bus_socket").is_err() {
        return Err("the guard lost its exact system-bus channel".to_owned());
    }
    if TcpListener::bind("127.0.0.1:0").is_ok() {
        return Err("the process created an AF_INET socket".to_owned());
    }

    for name in [
        "DISPLAY",
        "XAUTHORITY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "SSH_AUTH_SOCK",
    ] {
        if env::var_os(name).is_some() {
            return Err(format!("sensitive environment variable {name} survived"));
        }
    }

    // This replaces the current process rather than forking. Reaching the
    // success marker proves the no-exec mount rejected the direct execve.
    let error = Command::new("/usr/bin/true").exec();
    if error.raw_os_error().is_none() {
        return Err("the supplied executable failure was unclassified".to_owned());
    }
    Ok(())
}

fn required(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("missing probe variable {name}"))
}

fn read_to_string(path: &str) -> Result<String, String> {
    let mut value = String::new();
    File::open(path)
        .and_then(|mut file| file.read_to_string(&mut value))
        .map_err(|error| format!("cannot read inherited evidence: {error}"))?;
    Ok(value)
}
