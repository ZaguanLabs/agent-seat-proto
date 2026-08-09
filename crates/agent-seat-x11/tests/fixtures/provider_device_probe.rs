//! Hostile probe for provider-private and application-normal device namespaces.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, ExitCode};

const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";

fn main() -> ExitCode {
    let result = match env::args().nth(1).as_deref() {
        Some("provider") => provider(),
        Some("application") => application(),
        _ => Err("expected provider or application role".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agent-seat-provider-device-probe: {error}");
            ExitCode::FAILURE
        }
    }
}

fn provider() -> Result<(), String> {
    for path in ["/dev/input", "/dev/uinput"] {
        if fs::symlink_metadata(path).is_ok() || File::open(path).is_ok() {
            return Err(format!("provider retained input authority at {path}"));
        }
    }

    let executable = env::current_exe().map_err(|error| format!("current executable: {error}"))?;
    let marker = required("AGENT_SEAT_DEVICE_MARKER")?;
    let baseline = required("AGENT_SEAT_UINPUT_BASELINE")?;
    let mut command = Command::new(SYSTEMD_RUN);
    command.args([
        "--user",
        "--wait",
        "--collect",
        "--quiet",
        "--service-type=exec",
    ]);
    command.arg(format!(
        "--unit=agent-seat-device-application-{}.service",
        std::process::id()
    ));
    command.arg(format!("--setenv=AGENT_SEAT_DEVICE_MARKER={marker}"));
    command.arg(format!("--setenv=AGENT_SEAT_UINPUT_BASELINE={baseline}"));
    command.arg("--").arg(executable).arg("application");
    let output = command
        .output()
        .map_err(|error| format!("cannot delegate application: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "delegated application failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let evidence = fs::read_to_string(marker)
        .map_err(|error| format!("cannot read application evidence: {error}"))?;
    if evidence != "application-normal-device-namespace\n" {
        return Err("delegated application evidence changed".to_owned());
    }
    println!("agent-seat-provider-device-probe=pass");
    Ok(())
}

fn application() -> Result<(), String> {
    for path in ["/dev/input", "/dev/uinput"] {
        if fs::symlink_metadata(path).is_err() {
            return Err(format!("application inherited hidden device path {path}"));
        }
    }
    let expected_open = required("AGENT_SEAT_UINPUT_BASELINE")? == "open";
    if File::open("/dev/uinput").is_ok() != expected_open {
        return Err("application uinput access differs from its user-session baseline".to_owned());
    }
    let marker = required("AGENT_SEAT_DEVICE_MARKER")?;
    fs::write(
        Path::new(&marker),
        "application-normal-device-namespace\n",
    )
    .map_err(|error| format!("cannot write application evidence: {error}"))
}

fn required(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("missing {name}"))
}
