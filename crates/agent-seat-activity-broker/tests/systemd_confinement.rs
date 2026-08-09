//! Explicit rootless hostile probe for the production systemd sandbox.

#![cfg(target_os = "linux")]

use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use rustix::process::geteuid;

struct Fixture(PathBuf);

struct FileFixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        let path = PathBuf::from(format!(
            "/run/user/{}/agent-seat-confinement-{}-{label}",
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

impl Drop for FileFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
#[ignore = "requires an active systemd user manager; run explicitly for the deployment gate"]
fn runtime_profiles_deny_unowned_authority_but_preserve_exact_inherited_access() {
    run_confinement_probe(Manager::User);
}

#[test]
#[ignore = "requires explicit passwordless sudo and systemd; runs transient units only"]
fn production_identities_deny_unowned_authority_under_the_system_manager() {
    run_confinement_probe(Manager::System);
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Manager {
    User,
    System,
}

fn run_confinement_probe(manager: Manager) {
    let label = match manager {
        Manager::User => "user",
        Manager::System => "system",
    };
    let fixture = Fixture::new(label);
    let probe_source = fixture.0.join("probe");
    let probe = PathBuf::from(format!(
        "/tmp/agent-seat-confinement-probe-{}-{label}",
        std::process::id(),
    ));
    let probe_cleanup = FileFixture(probe.clone());
    let inherited = fixture.0.join("inherited");
    let runtime_secret = fixture.0.join("runtime-secret");
    let host_socket = fixture.0.join("host.sock");
    let child_socket = fixture.0.join("child.sock");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate is in the workspace crates directory");
    let home_secret = workspace
        .join("target")
        .join(format!("confinement-secret-{}-{label}", std::process::id()));
    let home_secret_cleanup = FileFixture(home_secret.clone());

    fs::write(&inherited, "inherited-evidence\n").expect("write inherited fixture");
    fs::write(&runtime_secret, "private\n").expect("write runtime fixture");
    fs::write(&home_secret, "private\n").expect("write home fixture");
    let _listener =
        std::os::unix::net::UnixListener::bind(&host_socket).expect("create host socket fixture");

    let compile = Command::new("rustc")
        .args(["--edition=2024", "-o"])
        .arg(&probe_source)
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/confinement_probe.rs"))
        .output()
        .expect("rustc is required for the explicit confinement gate");
    assert!(
        compile.status.success(),
        "cannot compile confinement probe: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    fs::copy(&probe_source, &probe).expect("stage probe at its executable path");
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).expect("make probe executable");

    // The transient D-Bus property uses an empty family list for the unit-file
    // spelling `none`.
    for (profile, families) in [("broker", ""), ("guard", "AF_UNIX AF_NETLINK")] {
        let mut command = match manager {
            Manager::User => Command::new("systemd-run"),
            Manager::System => {
                let mut command = Command::new("/usr/bin/sudo");
                command.args(["-n", "/usr/bin/systemd-run"]);
                command
            }
        };
        if manager == Manager::User {
            command.arg("--user");
        }
        command.args(["--wait", "--collect", "--pipe"]);
        if manager == Manager::System {
            command.arg("--property=DevicePolicy=strict");
            command.arg(if profile == "broker" {
                "--property=DynamicUser=yes"
            } else {
                "--property=User=agent-seat-guard"
            });
            for property in [
                "RemoveIPC=yes",
                "PrivateTmp=yes",
                "ProtectClock=yes",
                "ProtectControlGroups=yes",
                "ProtectHostname=yes",
                "ProtectKernelLogs=yes",
                "ProtectKernelModules=yes",
                "ProtectKernelTunables=yes",
                "MemoryMax=32M",
                "CPUQuota=10%",
            ] {
                command.arg("--property").arg(property);
            }
        }
        for property in [
            "PrivateDevices=yes".to_owned(),
            "PrivateNetwork=yes".to_owned(),
            format!("RestrictAddressFamilies={families}"),
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
            "ProtectSystem=strict".to_owned(),
            "ProtectHome=yes".to_owned(),
            "ProtectProc=invisible".to_owned(),
            "ProcSubset=pid".to_owned(),
            "TemporaryFileSystem=/run:ro".to_owned(),
            "NoExecPaths=/".to_owned(),
            format!(
                "ExecPaths={} -/usr/lib -/usr/lib64 -/lib -/lib64",
                path(&probe)
            ),
            format!(
                "BindReadOnlyPaths={}:{}",
                path(&probe_source),
                path(&probe)
            ),
            format!("OpenFile={}:evidence:read-only", path(&inherited)),
            "UnsetEnvironment=DISPLAY XAUTHORITY WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS SSH_AUTH_SOCK".to_owned(),
            "TasksMax=2".to_owned(),
            format!(
                "LimitNOFILE={}",
                if profile == "broker" { 64 } else { 32 }
            ),
            "LimitCORE=0".to_owned(),
        ] {
            command.arg("--property").arg(property);
        }
        if profile == "guard" {
            command
                .arg("--property")
                .arg("BindReadOnlyPaths=/run/dbus/system_bus_socket");
        }
        for (name, value) in [
            ("AGENT_SEAT_HOME_SECRET", path(&home_secret).to_owned()),
            (
                "AGENT_SEAT_RUNTIME_SECRET",
                path(&runtime_secret).to_owned(),
            ),
            (
                "AGENT_SEAT_PARENT_ENVIRON",
                format!("/proc/{}/environ", std::process::id()),
            ),
            ("AGENT_SEAT_HOST_SOCKET", path(&host_socket).to_owned()),
            ("AGENT_SEAT_CHILD_SOCKET", path(&child_socket).to_owned()),
        ] {
            command.arg(format!("--setenv={name}={value}"));
        }
        let output = command
            .arg(&probe)
            .arg(profile)
            .output()
            .expect("systemd-run and, for the system-manager gate, sudo are required");
        assert!(
            output.status.success(),
            "{profile} confinement probe failed with {}: stdout={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("agent-seat-confinement-probe=pass"),
            "the supplied executable replaced the {profile} probe"
        );
    }
    drop(home_secret_cleanup);
    drop(probe_cleanup);
}

fn path(path: &Path) -> &str {
    path.to_str().expect("fixture path is UTF-8")
}
