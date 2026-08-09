//! Hostile probe derived from the exact installed system-unit confinement.

#![cfg(target_os = "linux")]

use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::geteuid;

const SYSTEM_UNIT_DIRECTORY: &str = "/etc/systemd/system";
const RUNTIME_UNIT_DIRECTORY: &str = "/run/systemd/system";

#[test]
#[ignore = "requires installed enrollment, explicit passwordless sudo, and systemd"]
fn installed_unit_sandboxes_deny_unowned_authority_with_only_test_plumbing_replaced() {
    let uid = geteuid().as_raw();
    assert_ne!(uid, 0, "run the test as the enrolled desktop user");
    let fixture = ProbeFixture::new(uid);

    for profile in [Profile::Broker, Profile::Guard] {
        let installed_name = profile.installed_name(uid);
        let source = read_installed_unit(&installed_name);
        let runtime_name = format!(
            "agent-seat-installed-confinement-{}-{}.service",
            profile.label(),
            std::process::id()
        );
        let rendered = render_probe_unit(&source, profile, uid, &fixture);
        let source_path = fixture.root.join(&runtime_name);
        fs::write(&source_path, rendered).expect("write private runtime unit source");
        fs::set_permissions(&source_path, fs::Permissions::from_mode(0o600))
            .expect("protect runtime unit source");

        let unit = RuntimeUnit::install(&runtime_name, &source_path);
        unit.start_and_require_success();
        drop(unit);
    }
}

#[derive(Clone, Copy)]
enum Profile {
    Broker,
    Guard,
}

impl Profile {
    const fn label(self) -> &'static str {
        match self {
            Self::Broker => "broker",
            Self::Guard => "guard",
        }
    }

    fn installed_name(self, uid: u32) -> String {
        match self {
            Self::Broker => format!("agent-seat-activity-broker-{uid}.service"),
            Self::Guard => format!("agent-seat-eligibility-guard-{uid}.service"),
        }
    }
}

struct ProbeFixture {
    root: PathBuf,
    probe_source: PathBuf,
    probe_target: PathBuf,
    inherited: PathBuf,
    home_secret: PathBuf,
    runtime_secret: PathBuf,
    host_socket: PathBuf,
    child_socket: PathBuf,
    _host_listener: UnixListener,
}

impl ProbeFixture {
    fn new(uid: u32) -> Self {
        let root = PathBuf::from(format!(
            "/run/user/{uid}/agent-seat-installed-confinement-{}",
            std::process::id()
        ));
        let mut builder = DirBuilder::new();
        builder
            .mode(0o700)
            .create(&root)
            .expect("create private installed-confinement fixture");

        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crate is in the workspace crates directory");
        let probe_source = root.join("probe");
        let probe_target = PathBuf::from(format!(
            "/tmp/agent-seat-installed-confinement-probe-{}",
            std::process::id()
        ));
        let inherited = root.join("inherited");
        let runtime_secret = root.join("runtime-secret");
        let home_secret = workspace.join("target").join(format!(
            "installed-confinement-secret-{}",
            std::process::id()
        ));
        let host_socket = root.join("host.sock");
        let child_socket = root.join("child.sock");

        fs::write(&inherited, "inherited-evidence\n").expect("write inherited fixture");
        fs::write(&runtime_secret, "private\n").expect("write runtime fixture");
        fs::write(&home_secret, "private\n").expect("write home fixture");
        let host_listener = UnixListener::bind(&host_socket).expect("create host socket fixture");

        let compile = Command::new("rustc")
            .args(["--edition=2024", "-o"])
            .arg(&probe_source)
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/confinement_probe.rs"),
            )
            .output()
            .expect("rustc is required for the installed confinement gate");
        assert_success(&compile, "compile installed confinement probe");
        fs::copy(&probe_source, &probe_target).expect("stage installed confinement probe");
        fs::set_permissions(&probe_target, fs::Permissions::from_mode(0o755))
            .expect("make installed confinement probe executable");

        Self {
            root,
            probe_source,
            probe_target,
            inherited,
            home_secret,
            runtime_secret,
            host_socket,
            child_socket,
            _host_listener: host_listener,
        }
    }
}

impl Drop for ProbeFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.probe_target);
        let _ = fs::remove_file(&self.home_secret);
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct RuntimeUnit {
    name: String,
    path: PathBuf,
}

impl RuntimeUnit {
    fn install(name: &str, source: &Path) -> Self {
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte)),
            "runtime unit name is not canonical"
        );
        let path = Path::new(RUNTIME_UNIT_DIRECTORY).join(name);
        let absent = sudo_output(&["/usr/bin/test", "!", "-e", path_str(&path)]);
        assert_success(&absent, "refuse an existing runtime unit target");
        let install = sudo_output(&[
            "/usr/bin/install",
            "-o",
            "root",
            "-g",
            "root",
            "-m",
            "0600",
            path_str(source),
            path_str(&path),
        ]);
        assert_success(&install, "install volatile hostile unit");
        let unit = Self {
            name: name.to_owned(),
            path,
        };
        let reload = sudo_output(&["/usr/bin/systemctl", "daemon-reload"]);
        assert_success(&reload, "reload volatile hostile unit");
        unit
    }

    fn start_and_require_success(&self) {
        let start = sudo_output(&["/usr/bin/systemctl", "start", &self.name]);
        assert_success(&start, "start volatile hostile unit");

        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let state = systemctl_value(&self.name, "ActiveState");
            if matches!(state.as_str(), "inactive" | "failed") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "volatile hostile unit did not terminate within its bound"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(systemctl_value(&self.name, "Result"), "success");
        assert_eq!(systemctl_value(&self.name, "ExecMainStatus"), "0");
    }
}

impl Drop for RuntimeUnit {
    fn drop(&mut self) {
        let _ = sudo_output(&["/usr/bin/systemctl", "stop", &self.name]);
        let _ = sudo_output(&["/usr/bin/rm", "-f", "--", path_str(&self.path)]);
        let _ = sudo_output(&["/usr/bin/systemctl", "daemon-reload"]);
    }
}

fn read_installed_unit(name: &str) -> String {
    let path = Path::new(SYSTEM_UNIT_DIRECTORY).join(name);
    let output = sudo_output(&["/usr/bin/cat", path_str(&path)]);
    assert_success(&output, "read installed unit");
    String::from_utf8(output.stdout).expect("installed unit is UTF-8")
}

fn render_probe_unit(source: &str, profile: Profile, uid: u32, fixture: &ProbeFixture) -> String {
    assert!(
        source.ends_with('\n'),
        "installed unit is not newline terminated"
    );
    let mut output = String::with_capacity(source.len() + 1024);
    let mut requires = 0;
    let mut after = 0;
    let mut collect = 0;
    let mut standard_input = 0;
    let mut standard_output = 0;
    let mut exec_start = 0;
    let mut open_files = 0;
    let mut exec_paths = 0;

    for line in source.lines() {
        if line.starts_with("Requires=") {
            requires += 1;
            continue;
        }
        if line.starts_with("After=") {
            after += 1;
            continue;
        }
        if line == "CollectMode=inactive-or-failed" {
            collect += 1;
            continue;
        }
        if line.starts_with("StandardInput=") {
            standard_input += 1;
            output.push_str(&format!(
                "StandardInput=file:{}\n",
                path_str(&fixture.host_socket)
            ));
            continue;
        }
        if line.starts_with("StandardOutput=") {
            standard_output += 1;
            output.push_str("StandardOutput=journal\n");
            continue;
        }
        if line.starts_with("ExecStart=") {
            exec_start += 1;
            output.push_str(&format!(
                "ExecStart={} {}\n",
                path_str(&fixture.probe_target),
                profile.label()
            ));
            continue;
        }
        if line.starts_with("OpenFile=") {
            open_files += 1;
            if open_files == 1 {
                output.push_str(&format!(
                    "OpenFile={}:evidence:read-only\n",
                    path_str(&fixture.inherited)
                ));
            }
            continue;
        }
        if line.starts_with("ExecPaths=") {
            exec_paths += 1;
            output.push_str(&format!(
                "ExecPaths={} -/usr/lib -/usr/lib64 -/lib -/lib64\n",
                path_str(&fixture.probe_target)
            ));
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }

    assert_eq!(
        requires,
        match profile {
            Profile::Broker => 2,
            Profile::Guard => 1,
        },
        "installed unit requires shape changed"
    );
    assert_eq!(after, 1, "installed unit ordering shape changed");
    assert_eq!(collect, 1, "installed unit collection shape changed");
    assert_eq!(standard_input, 1, "installed unit stdin shape changed");
    assert_eq!(standard_output, 1, "installed unit stdout shape changed");
    assert_eq!(exec_start, 1, "installed unit executable shape changed");
    assert!(
        open_files >= 1,
        "installed unit inherited-file shape changed"
    );
    assert_eq!(exec_paths, 1, "installed unit executable policy changed");

    for (name, value) in [
        (
            "AGENT_SEAT_HOME_SECRET",
            path_str(&fixture.home_secret).to_owned(),
        ),
        (
            "AGENT_SEAT_RUNTIME_SECRET",
            path_str(&fixture.runtime_secret).to_owned(),
        ),
        (
            "AGENT_SEAT_PARENT_ENVIRON",
            format!("/proc/{}/environ", std::process::id()),
        ),
        (
            "AGENT_SEAT_HOST_SOCKET",
            path_str(&fixture.host_socket).to_owned(),
        ),
        (
            "AGENT_SEAT_CHILD_SOCKET",
            path_str(&fixture.child_socket).to_owned(),
        ),
    ] {
        assert_unit_value(&value);
        output.push_str(&format!("Environment={name}={value}\n"));
    }
    output.push_str(&format!(
        "BindReadOnlyPaths={}:{}\n",
        path_str(&fixture.probe_source),
        path_str(&fixture.probe_target)
    ));
    output.push_str(&format!("# enrolled-uid={uid}\n"));
    output
}

fn systemctl_value(unit: &str, property: &str) -> String {
    let output = Command::new("systemctl")
        .args(["show", unit, "--property", property, "--value"])
        .output()
        .expect("systemctl is required for the installed confinement gate");
    assert_success(&output, "inspect volatile hostile unit");
    String::from_utf8(output.stdout)
        .expect("systemctl output is UTF-8")
        .trim()
        .to_owned()
}

fn sudo_output(arguments: &[&str]) -> Output {
    Command::new("/usr/bin/sudo")
        .arg("-n")
        .args(arguments)
        .output()
        .expect("fixed /usr/bin/sudo is required for the installed confinement gate")
}

fn assert_success(output: &Output, action: &str) {
    assert!(
        output.status.success(),
        "cannot {action}: status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_unit_value(value: &str) {
    assert!(
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\')),
        "fixture path cannot be represented as one systemd environment value"
    );
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("fixture path is UTF-8")
}
