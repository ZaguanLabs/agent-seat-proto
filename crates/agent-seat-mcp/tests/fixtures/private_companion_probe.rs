//! Hostile process probe for the optional private MCP profile.

#![forbid(unsafe_code)]

use std::env;
use std::fs::File;
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match probe() {
        Ok(()) => {
            println!("agent-seat-private-companion-probe=pass");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("agent-seat-private-companion-probe: {error}");
            ExitCode::FAILURE
        }
    }
}

fn probe() -> Result<(), String> {
    for path in ["/dev/input", "/dev/uinput"] {
        if File::open(path).is_ok() || std::fs::symlink_metadata(path).is_ok() {
            return Err(format!("input authority remained visible at {path}"));
        }
    }
    for (label, path) in [
        ("home", required("AGENT_SEAT_HOME_SECRET")?),
        ("parent process", required("AGENT_SEAT_PARENT_ENVIRON")?),
    ] {
        if File::open(path).is_ok() {
            return Err(format!("{label} metadata remained readable"));
        }
    }
    let process_id = std::process::id().to_string();
    if env::var("LISTEN_PID").ok().as_deref() != Some(process_id.as_str())
        || env::var("LISTEN_FDS").ok().as_deref() != Some("1")
        || env::var("LISTEN_FDNAMES").ok().as_deref() != Some("agent-seat-provider")
    {
        return Err("the exact named provider descriptor was not addressed here".to_owned());
    }
    let provider_link = std::fs::read_link("/proc/self/fd/3")
        .map_err(|error| format!("provider descriptor is unavailable: {error}"))?;
    if !provider_link.to_string_lossy().starts_with("socket:[") {
        return Err("the inherited provider descriptor is not a socket".to_owned());
    }
    if UnixStream::connect(required("AGENT_SEAT_FORBIDDEN_SOCKET")?).is_ok() {
        return Err("an unbound broker-like socket remained reachable".to_owned());
    }
    if UnixStream::connect("/tmp/.X11-unix/X0").is_ok() {
        return Err("an X11 socket remained reachable".to_owned());
    }
    if TcpListener::bind("127.0.0.1:0").is_ok() {
        return Err("an IP socket remained available".to_owned());
    }
    for name in [
        "DISPLAY",
        "XAUTHORITY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "SSH_AUTH_SOCK",
        "AGENT_SEAT_SOCKET",
        "HOME",
        "PATH",
    ] {
        if env::var_os(name).is_some() {
            return Err(format!("environment variable {name} survived"));
        }
    }

    let error = Command::new("/usr/bin/true").exec();
    if error.raw_os_error().is_none() {
        return Err("the denied direct execution failure was unclassified".to_owned());
    }
    Ok(())
}

fn required(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("missing {name}"))
}
