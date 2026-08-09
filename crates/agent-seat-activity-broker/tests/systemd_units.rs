//! Syntax gate for the inert enrollment-rendered systemd unit sources.

use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::process::geteuid;

const SERVICE_SOURCE: &str =
    include_str!("../../../contrib/systemd/agent-seat-activity-broker.service.in");
const SOCKET_SOURCE: &str =
    include_str!("../../../contrib/systemd/agent-seat-activity-broker.socket.in");
const GUARD_SERVICE_SOURCE: &str =
    include_str!("../../../contrib/systemd/agent-seat-eligibility-guard.service.in");
const GUARD_SOCKET_SOURCE: &str =
    include_str!("../../../contrib/systemd/agent-seat-eligibility-guard.socket.in");
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-seat-broker-units-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = DirBuilder::new();
        builder.mode(0o700).create(&path).expect("unit fixture");
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn rendered_units_are_inert_bounded_and_accepted_by_systemd() {
    let fixture = Fixture::new();
    let eligibility = fixture.0.join("eligibility.sock");
    let event = PathBuf::from("/dev/null");
    let input_set = fixture.0.join("initial-input-set.v1");
    let device_set = fixture.0.join("enrolled-device-set.v1");
    fs::write(&eligibility, []).expect("eligibility fixture");
    fs::write(&input_set, []).expect("input-set fixture");
    fs::write(&device_set, []).expect("device-set fixture");

    let stem = "agent-seat-activity-broker-test";
    let socket_name = format!("{stem}.socket");
    let guard_stem = "agent-seat-eligibility-guard-test";
    let guard_service_name = format!("{guard_stem}.service");
    let guard_socket_name = format!("{guard_stem}.socket");
    let service_path = fixture.0.join(format!("{stem}.service"));
    let socket_path = fixture.0.join(&socket_name);
    let guard_service_path = fixture.0.join(&guard_service_name);
    let guard_socket_path = fixture.0.join(&guard_socket_name);
    let provider_uid = geteuid().as_raw();
    let service = SERVICE_SOURCE
        .replace("@SOCKET_UNIT@", &socket_name)
        .replace("@ELIGIBILITY_SOCKET_UNIT@", &guard_socket_name)
        .replace(
            "@BROKER_EXEC@",
            env!("CARGO_BIN_EXE_agent-seat-activity-broker"),
        )
        .replace("@PROVIDER_UID@", &provider_uid.to_string())
        .replace("@ELIGIBILITY_PATH@", path(&eligibility))
        .replace("@DEVICE_SET_PATH@", path(&device_set))
        .replace(
            "@EVENT_OPEN_FILES@",
            &format!("OpenFile={}:event0:read-only", path(&event)),
        )
        .replace(
            "@EVENT_DEVICE_ALLOW@",
            &format!("DeviceAllow={} r", path(&event)),
        );
    let socket = SOCKET_SOURCE
        .replace("@BROKER_SOCKET@", path(&fixture.0.join("broker.sock")))
        .replace("@PROVIDER_UID@", &provider_uid.to_string());
    let guard_service = GUARD_SERVICE_SOURCE
        .replace("@ELIGIBILITY_SOCKET_UNIT@", &guard_socket_name)
        .replace(
            "@GUARD_EXEC@",
            env!("CARGO_BIN_EXE_agent-seat-eligibility-guard"),
        )
        .replace("@GUARD_USER@", &provider_uid.to_string())
        .replace("@SESSION_ID@", "test-session")
        .replace("@INPUT_SET_PATH@", path(&input_set))
        .replace("@PROVIDER_UID@", &provider_uid.to_string());
    let guard_socket = GUARD_SOCKET_SOURCE
        .replace("@ELIGIBILITY_SOCKET@", path(&eligibility))
        .replace("@ELIGIBILITY_SERVICE_UNIT@", &guard_service_name);
    for placeholder in [
        "@SOCKET_UNIT@",
        "@BROKER_EXEC@",
        "@PROVIDER_UID@",
        "@ELIGIBILITY_PATH@",
        "@EVENT_OPEN_FILES@",
        "@EVENT_DEVICE_ALLOW@",
        "@DEVICE_SET_PATH@",
        "@BROKER_SOCKET@",
        "@ELIGIBILITY_SOCKET_UNIT@",
        "@ELIGIBILITY_SERVICE_UNIT@",
        "@ELIGIBILITY_SOCKET@",
        "@GUARD_EXEC@",
        "@GUARD_USER@",
        "@SESSION_ID@",
        "@INPUT_SET_PATH@",
    ] {
        assert!(
            !service.contains(placeholder)
                && !socket.contains(placeholder)
                && !guard_service.contains(placeholder)
                && !guard_socket.contains(placeholder),
            "unrendered unit placeholder {placeholder}"
        );
    }
    assert!(
        !socket.lines().any(|line| line.trim() == "[Install]"),
        "socket became enableable"
    );
    for required in [
        "DynamicUser=yes",
        "StartLimitIntervalSec=infinity",
        "StartLimitBurst=1",
        "StandardOutput=socket",
        "DevicePolicy=strict",
        "PrivateDevices=yes",
        "PrivateNetwork=yes",
        "RestrictAddressFamilies=none",
        "NoNewPrivileges=yes",
        "CapabilityBoundingSet=",
        "AmbientCapabilities=",
        "ProtectSystem=strict",
        "ProtectHome=yes",
        "ProtectProc=invisible",
        "TemporaryFileSystem=/run:ro",
        "NoExecPaths=/",
        &format!(
            "ExecPaths={} -/usr/lib -/usr/lib64 -/lib -/lib64",
            env!("CARGO_BIN_EXE_agent-seat-activity-broker")
        ),
        "TasksMax=2",
        "LimitCORE=0",
        "Restart=no",
    ] {
        assert!(
            has_line(&service, required),
            "missing service gate {required}"
        );
    }
    let open_files = service
        .lines()
        .filter(|line| line.starts_with("OpenFile="))
        .collect::<Vec<_>>();
    assert_eq!(open_files.len(), 2);
    assert!(open_files.iter().all(|line| line.ends_with(":read-only")));
    assert!(has_line(
        &service,
        &format!("StandardInput=file:{}", path(&eligibility))
    ));
    assert!(has_line(
        &service,
        &format!(
            "ExecStart={} --uid {}",
            env!("CARGO_BIN_EXE_agent-seat-activity-broker"),
            provider_uid
        )
    ));
    assert!(has_line(
        &service,
        &format!(
            "OpenFile={}:enrolled-device-set:read-only",
            path(&device_set)
        )
    ));
    assert_eq!(
        service
            .lines()
            .filter(|line| line.starts_with("DeviceAllow="))
            .collect::<Vec<_>>(),
        [format!("DeviceAllow={} r", path(&event))]
    );
    assert!(!service.contains("ExecStartPre="));
    assert!(has_line(&socket, "SocketMode=0600"));
    assert!(has_line(&socket, "DirectoryMode=0711"));
    assert!(has_line(&socket, "Accept=no"));
    assert!(has_line(&socket, "Backlog=1"));
    assert!(has_line(&service, &format!("Requires={guard_socket_name}")));
    for required in [
        &format!("User={provider_uid}"),
        "StandardInput=socket",
        "DevicePolicy=strict",
        "PrivateDevices=yes",
        "PrivateNetwork=yes",
        "RestrictAddressFamilies=AF_UNIX AF_NETLINK",
        "NoNewPrivileges=yes",
        "CapabilityBoundingSet=",
        "ProtectSystem=strict",
        "ProtectHome=yes",
        "TemporaryFileSystem=/run:ro",
        "BindReadOnlyPaths=/run/dbus/system_bus_socket",
        "NoExecPaths=/",
        &format!(
            "ExecPaths={} -/usr/lib -/usr/lib64 -/lib -/lib64",
            env!("CARGO_BIN_EXE_agent-seat-eligibility-guard")
        ),
        "TasksMax=2",
        "Restart=no",
    ] {
        assert!(
            has_line(&guard_service, required),
            "missing eligibility guard gate {required}"
        );
    }
    assert!(!has_line(&guard_service, "DynamicUser=yes"));
    assert_eq!(
        guard_service
            .lines()
            .filter(|line| line.starts_with("OpenFile="))
            .count(),
        1
    );
    assert!(!guard_service.contains("DeviceAllow="));
    assert!(has_line(
        &guard_service,
        &format!("OpenFile={}:initial-input-set:read-only", path(&input_set))
    ));
    assert!(has_line(&guard_socket, "SocketMode=0600"));
    assert!(has_line(&guard_socket, "DirectoryMode=0711"));
    assert!(has_line(&guard_socket, "Accept=no"));
    assert!(has_line(&guard_socket, "Backlog=1"));
    assert!(has_line(
        &guard_socket,
        &format!("Service={guard_service_name}")
    ));
    fs::write(&service_path, service).expect("render service fixture");
    fs::write(&socket_path, socket).expect("render socket fixture");
    fs::write(&guard_service_path, guard_service).expect("render guard service fixture");
    fs::write(&guard_socket_path, guard_socket).expect("render guard socket fixture");

    let output = Command::new("systemd-analyze")
        .args(["verify", "--man=no"])
        .arg(&service_path)
        .arg(&socket_path)
        .arg(&guard_service_path)
        .arg(&guard_socket_path)
        .output()
        .expect("systemd-analyze is required by the broker deployment gate");
    assert!(
        output.status.success(),
        "systemd rejected broker units: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "systemd emitted unit diagnostics: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path(path: &Path) -> &str {
    path.to_str().expect("fixture path is UTF-8")
}

fn has_line(source: &str, exact: &str) -> bool {
    source.lines().any(|line| line.trim() == exact)
}
