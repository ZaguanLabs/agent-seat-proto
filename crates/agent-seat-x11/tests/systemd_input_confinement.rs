//! Explicit live gate for provider device confinement and launch delegation.

#![cfg(target_os = "linux")]

use std::env;
use std::fs::{self, DirBuilder, File};
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use rustix::process::geteuid;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/run/user/{}/agent-seat-provider-confinement-{}",
            geteuid().as_raw(),
            std::process::id()
        ));
        let mut builder = DirBuilder::new();
        builder
            .mode(0o700)
            .create(&path)
            .expect("create private runtime fixture");
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires an active systemd user manager and host input nodes; run explicitly for the deployment gate"]
fn provider_loses_input_devices_while_delegated_application_keeps_baseline() {
    assert!(Path::new("/dev/input").exists(), "host has no /dev/input");
    assert!(Path::new("/dev/uinput").exists(), "host has no /dev/uinput");
    let fixture = Fixture::new();
    let probe = fixture.0.join("provider-device-probe");
    let marker = fixture.0.join("application-evidence");
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/provider_device_probe.rs");
    let compile = Command::new("rustc")
        .args(["--edition=2024", "-o"])
        .arg(&probe)
        .arg(source)
        .output()
        .expect("rustc is required for the explicit provider confinement gate");
    assert!(
        compile.status.success(),
        "cannot compile provider probe: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let baseline = if File::open("/dev/uinput").is_ok() {
        "open"
    } else {
        "denied"
    };
    let mut command = Command::new("systemd-run");
    command.args(["--user", "--wait", "--collect", "--pipe"]);
    for property in [
        "PrivateDevices=yes".to_owned(),
        "DevicePolicy=strict".to_owned(),
        "RestrictAddressFamilies=AF_UNIX".to_owned(),
        "SystemCallArchitectures=native".to_owned(),
        "SystemCallFilter=@system-service".to_owned(),
        "SystemCallErrorNumber=EPERM".to_owned(),
        "NoNewPrivileges=yes".to_owned(),
        "CapabilityBoundingSet=".to_owned(),
        "AmbientCapabilities=".to_owned(),
        "LockPersonality=yes".to_owned(),
        "MemoryDenyWriteExecute=yes".to_owned(),
        "RestrictNamespaces=yes".to_owned(),
        "RestrictRealtime=yes".to_owned(),
        "RestrictSUIDSGID=yes".to_owned(),
        "RemoveIPC=yes".to_owned(),
        "ProtectSystem=strict".to_owned(),
        "ProtectHome=read-only".to_owned(),
        "PrivateTmp=yes".to_owned(),
        "BindReadOnlyPaths=/tmp/.X11-unix".to_owned(),
        "ProtectProc=invisible".to_owned(),
        "ProcSubset=pid".to_owned(),
        "ProtectClock=yes".to_owned(),
        "ProtectControlGroups=yes".to_owned(),
        "ProtectHostname=yes".to_owned(),
        "ProtectKernelLogs=yes".to_owned(),
        "ProtectKernelModules=yes".to_owned(),
        "ProtectKernelTunables=yes".to_owned(),
        "InaccessiblePaths=-/dev/input -/dev/uinput".to_owned(),
        format!("ReadWritePaths={}", path(&fixture.0)),
        "NoExecPaths=/".to_owned(),
        format!(
            "ExecPaths={} /usr/bin/systemd-run -/usr/lib -/usr/lib64 -/lib -/lib64",
            path(&probe)
        ),
        "UMask=0077".to_owned(),
        "TasksMax=128".to_owned(),
        "LimitNOFILE=256".to_owned(),
        "LimitCORE=0".to_owned(),
        "MemoryMax=256M".to_owned(),
    ] {
        command.arg("--property").arg(property);
    }
    command.arg(format!(
        "--setenv=AGENT_SEAT_DEVICE_MARKER={}",
        path(&marker)
    ));
    command.arg(format!("--setenv=AGENT_SEAT_UINPUT_BASELINE={baseline}"));
    for name in ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"] {
        if let Some(value) = env::var_os(name) {
            command.arg(format!("--setenv={name}={}", value.to_string_lossy()));
        }
    }
    let output = command
        .arg(&probe)
        .arg("provider")
        .output()
        .expect("systemd-run is required for the provider confinement gate");
    assert!(
        output.status.success(),
        "provider confinement probe failed with {}: stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("agent-seat-provider-device-probe=pass"),
        "the provider confinement probe did not reach its success marker"
    );
}

fn path(path: &Path) -> &str {
    path.to_str().expect("fixture path is UTF-8")
}
