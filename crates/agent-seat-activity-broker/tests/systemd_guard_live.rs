//! Explicit live-session guard probe under the hardened systemd profile.

#![cfg(target_os = "linux")]

use std::fs::{self, DirBuilder};
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use agent_seat_activity_broker::{EligibilityState, read_eligibility};
use rustix::process::geteuid;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/run/user/{}/agent-seat-guard-live-{}",
            geteuid().as_raw(),
            std::process::id()
        ));
        let mut builder = DirBuilder::new();
        builder
            .mode(0o700)
            .create(&path)
            .expect("create private live-guard fixture");
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct StagedExecutable(PathBuf);

impl Drop for StagedExecutable {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

struct TransientUnits(String);

impl Drop for TransientUnits {
    fn drop(&mut self) {
        let _ = Command::new("systemctl")
            .args(["--user", "stop"])
            .arg(format!("{}.socket", self.0))
            .arg(format!("{}.service", self.0))
            .status();
    }
}

#[test]
#[ignore = "requires an active local X11 session and systemd user manager"]
fn guard_reaches_real_logind_and_netlink_under_the_hardened_profile() {
    let session = std::env::var("XDG_SESSION_ID")
        .expect("XDG_SESSION_ID must identify the active test session");
    assert!(
        !session.is_empty()
            && session.len() <= 64
            && session
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte)),
        "XDG_SESSION_ID is outside the guard's bound"
    );
    let uid = geteuid().as_raw();
    assert_ne!(uid, 0, "the rootless guard probe needs an ordinary session");

    let fixture = Fixture::new();
    let bundle = fixture.0.join("bundle");
    let render = Command::new(env!("CARGO_BIN_EXE_agent-seat-activity-enroll"))
        .args(["render", "--uid", &uid.to_string(), "--session", &session])
        .arg("--output")
        .arg(&bundle)
        .output()
        .expect("run the read-only enrollment renderer");
    assert!(
        render.status.success(),
        "cannot render current input-set evidence: {}",
        String::from_utf8_lossy(&render.stderr)
    );

    let source = PathBuf::from(env!("CARGO_BIN_EXE_agent-seat-eligibility-guard"));
    let executable = PathBuf::from(format!(
        "/tmp/agent-seat-eligibility-guard-live-{}",
        std::process::id()
    ));
    let executable_cleanup = StagedExecutable(executable.clone());
    fs::copy(&source, &executable).expect("stage guard executable");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("protect staged guard executable");

    let stem = format!("agent-seat-guard-live-{}", std::process::id());
    let units = TransientUnits(stem.clone());
    let socket = fixture.0.join("eligibility.sock");
    let input_set = bundle.join("initial-input-set.v1");
    let mut command = Command::new("systemd-run");
    command
        .args(["--user", "--collect", "--no-block"])
        .arg("--unit")
        .arg(&stem)
        .arg("--property=Type=exec")
        .arg("--property=StandardInput=socket")
        .arg("--property=StandardOutput=journal")
        .arg("--property=StandardError=journal")
        .arg("--property=DevicePolicy=strict")
        .arg("--property=PrivateDevices=yes")
        .arg("--property=PrivateNetwork=yes")
        .arg("--property=RestrictAddressFamilies=AF_UNIX AF_NETLINK")
        .arg("--property=SystemCallArchitectures=native")
        .arg("--property=SystemCallFilter=@system-service")
        .arg("--property=SystemCallErrorNumber=EPERM")
        .arg("--property=NoNewPrivileges=yes")
        .arg("--property=CapabilityBoundingSet=")
        .arg("--property=AmbientCapabilities=")
        .arg("--property=LockPersonality=yes")
        .arg("--property=MemoryDenyWriteExecute=yes")
        .arg("--property=RestrictNamespaces=yes")
        .arg("--property=RestrictRealtime=yes")
        .arg("--property=RestrictSUIDSGID=yes")
        .arg("--property=ProtectSystem=strict")
        .arg("--property=ProtectHome=yes")
        .arg("--property=PrivateTmp=yes")
        .arg("--property=ProtectProc=invisible")
        .arg("--property=ProcSubset=pid")
        .arg("--property=TemporaryFileSystem=/run:ro")
        .arg("--property=BindReadOnlyPaths=/run/dbus/system_bus_socket")
        .arg("--property=NoExecPaths=/")
        .arg(format!(
            "--property=ExecPaths={} -/usr/lib -/usr/lib64 -/lib -/lib64",
            path(&executable)
        ))
        .arg(format!(
            "--property=BindReadOnlyPaths={}:{}",
            path(&source),
            path(&executable)
        ))
        .arg(format!(
            "--property=OpenFile={}:initial-input-set:read-only",
            path(&input_set)
        ))
        .arg("--property=UnsetEnvironment=DISPLAY XAUTHORITY WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS SSH_AUTH_SOCK")
        .arg("--property=TasksMax=2")
        .arg("--property=LimitNOFILE=32")
        .arg("--property=LimitCORE=0")
        .arg("--property=MemoryMax=32M")
        .arg("--property=CPUQuota=10%")
        .arg("--socket-property=SocketMode=0600")
        .arg("--socket-property=DirectoryMode=0700")
        .arg("--socket-property=Accept=no")
        .arg("--socket-property=Backlog=1")
        .arg(format!("--socket-property=ListenStream={}", path(&socket)))
        .arg(&executable)
        .args([
            "--session",
            &session,
            "--uid",
            &uid.to_string(),
            "--seat",
            "seat0",
            "--listen-stdin",
            "--peer-uid",
            &uid.to_string(),
        ]);
    let started = command
        .output()
        .expect("systemd-run is required for the live guard gate");
    assert!(
        started.status.success(),
        "cannot create transient guard units: {}",
        String::from_utf8_lossy(&started.stderr)
    );

    let mut stream = connect_bounded(&socket);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("bound eligibility read");
    assert_eq!(
        read_eligibility(&mut stream),
        Ok(EligibilityState::Eligible),
        "the current active local X11 session was not eligible"
    );

    drop(stream);
    drop(units);
    drop(executable_cleanup);
}

fn connect_bounded(path: &Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return stream,
            Err(error)
                if Instant::now() < deadline
                    && matches!(
                        error.kind(),
                        ErrorKind::NotFound | ErrorKind::ConnectionRefused
                    ) =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("cannot connect to transient guard: {error}"),
        }
    }
}

fn path(path: &Path) -> &str {
    path.to_str().expect("fixture path is UTF-8")
}
