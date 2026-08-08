//! Isolated GTK first-run and Openbox mapping test.

use std::fs::{self, DirBuilder};
use std::io::{BufRead as _, Read as _};
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-seat-settings-gui-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = DirBuilder::new();
        builder.mode(0o700).create(&path).expect("GUI fixture");
        Self(path)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Desktop {
    xvfb: Child,
    openbox: Child,
    display: String,
}

impl Desktop {
    fn start() -> Self {
        let mut xvfb = Command::new("Xvfb")
            .args([
                "-screen",
                "0",
                "1280x900x24",
                "-nolisten",
                "tcp",
                "-displayfd",
                "1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Xvfb is required by the Settings GUI gate");
        let mut display_number = String::new();
        std::io::BufReader::new(xvfb.stdout.take().expect("Xvfb display pipe"))
            .read_line(&mut display_number)
            .expect("Xvfb display number");
        assert!(!display_number.trim().is_empty(), "Xvfb did not start");
        let display = format!(":{}", display_number.trim());
        let openbox = Command::new("openbox")
            .env("DISPLAY", &display)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Openbox is required by the Settings GUI gate");
        Self {
            xvfb,
            openbox,
            display,
        }
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        let _ = self.openbox.kill();
        let _ = self.openbox.wait();
        let _ = self.xvfb.kill();
        let _ = self.xvfb.wait();
    }
}

struct SettingsProcess(Child);

impl Drop for SettingsProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn first_gui_run_maps_in_openbox_and_creates_only_a_disabled_private_policy() {
    let fixture = FixtureDir::new();
    let desktop = Desktop::start();
    let config_home = fixture.0.join("config");
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-seat-settings"))
        .env("DISPLAY", &desktop.display)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_RUNTIME_DIR", &fixture.0)
        .env("XDG_DATA_HOME", fixture.0.join("user-data"))
        .env("XDG_DATA_DIRS", fixture.0.join("system-data"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Settings GUI");

    wait_for_settings_window(&desktop.display, &mut child);
    let _settings = SettingsProcess(child);
    let policy = config_home.join("agent-seat/config.toml");
    let source = fs::read_to_string(&policy).expect("read first-run policy");
    assert!(source.contains("This file was created on the first run"));
    assert!(source.contains("enabled = false"));
    assert_eq!(
        fs::metadata(&policy)
            .expect("first-run policy metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(!fixture.0.join("agent-seat").exists());
}

fn wait_for_settings_window(display: &str, child: &mut Child) {
    let (connection, screen) = x11rb::connect(Some(display)).expect("GUI test X11 connection");
    let root = connection.setup().roots[screen].root;
    let clients = connection
        .intern_atom(false, b"_NET_CLIENT_LIST")
        .expect("client-list atom request")
        .reply()
        .expect("client-list atom reply")
        .atom;
    let name = connection
        .intern_atom(false, b"_NET_WM_NAME")
        .expect("window-name atom request")
        .reply()
        .expect("window-name atom reply")
        .atom;
    let utf8 = connection
        .intern_atom(false, b"UTF8_STRING")
        .expect("UTF-8 atom request")
        .reply()
        .expect("UTF-8 atom reply")
        .atom;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let client_list = connection
            .get_property(false, root, clients, AtomEnum::WINDOW, 0, 128)
            .expect("client-list request")
            .reply()
            .expect("client-list reply");
        let windows = client_list.value32().into_iter().flatten();
        for window in windows {
            let property = connection
                .get_property(false, window, name, utf8, 0, 128)
                .expect("window-name request")
                .reply()
                .expect("window-name reply");
            if property.value == b"Agent Seat Settings" {
                return;
            }
        }
        if let Some(status) = child.try_wait().expect("Settings GUI status") {
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .expect("Settings stderr")
                .read_to_string(&mut stderr)
                .expect("read Settings stderr");
            panic!("Settings GUI exited before mapping ({status}): {stderr}");
        }
        assert!(Instant::now() < deadline, "Settings window did not map");
        thread::sleep(Duration::from_millis(20));
    }
}
