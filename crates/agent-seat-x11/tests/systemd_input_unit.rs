//! Static and systemd syntax gates for the optional private-device user unit.

use std::fs;
use std::process::Command;

const UNIT_SOURCE: &str =
    include_str!("../../../contrib/systemd/user/agent-seat-x11-input.service");

#[test]
fn input_unit_is_explicit_bounded_and_accepted_by_systemd() {
    assert!(!UNIT_SOURCE.lines().any(|line| line.trim() == "[Install]"));
    assert!(!has_line(UNIT_SOURCE, "PrivateNetwork=yes"));
    for required in [
        "Type=exec",
        "ExecStart=/usr/bin/agent-seat-x11",
        "PrivateDevices=yes",
        "DevicePolicy=strict",
        "RestrictAddressFamilies=AF_UNIX",
        "NoNewPrivileges=yes",
        "CapabilityBoundingSet=",
        "AmbientCapabilities=",
        "ProtectSystem=strict",
        "ProtectHome=read-only",
        "PrivateTmp=yes",
        "BindReadOnlyPaths=/tmp/.X11-unix",
        "ProtectProc=invisible",
        "InaccessiblePaths=-/dev/input -/dev/uinput",
        "RuntimeDirectory=agent-seat",
        "RuntimeDirectoryMode=0700",
        "RuntimeDirectoryPreserve=yes",
        "ReadWritePaths=%t/agent-seat",
        "NoExecPaths=/",
        "ExecPaths=/usr/bin/agent-seat-x11 /usr/bin/systemd-run -/usr/lib -/usr/lib64 -/lib -/lib64",
        "TasksMax=128",
        "LimitNOFILE=256",
        "LimitCORE=0",
        "MemoryMax=256M",
        "Restart=no",
    ] {
        assert!(
            has_line(UNIT_SOURCE, required),
            "missing unit gate {required}"
        );
    }

    let directory =
        std::env::temp_dir().join(format!("agent-seat-x11-unit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir(&directory).expect("unit fixture directory");
    let unit = directory.join("agent-seat-x11-input.service");
    fs::write(&unit, UNIT_SOURCE).expect("unit fixture");
    let output = Command::new("systemd-analyze")
        .args(["verify", "--man=no"])
        .arg(&unit)
        .output()
        .expect("systemd-analyze is required by the provider deployment gate");
    let _ = fs::remove_dir_all(&directory);
    assert!(
        output.status.success(),
        "systemd rejected provider unit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "systemd emitted unit diagnostics: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn has_line(source: &str, exact: &str) -> bool {
    source.lines().any(|line| line.trim() == exact)
}
