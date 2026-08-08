//! Process-isolated tests for the public settings application catalog.

use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_seat_x11::installed_applications;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-seat-catalog-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = DirBuilder::new();
        builder
            .mode(0o700)
            .create(&path)
            .expect("fixture directory");
        Self(path)
    }

    fn application(&self, root: &str, id: &str, contents: &str) {
        let directory = self.0.join(root).join("applications");
        fs::create_dir_all(&directory).expect("application directory");
        fs::write(directory.join(id), contents).expect("desktop entry");
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn installed_catalog_helper() {
    let Some(result_path) = std::env::var_os("AGENT_SEAT_CATALOG_RESULT") else {
        return;
    };
    let applications = installed_applications().expect("discover installed applications");
    let result = applications
        .iter()
        .map(|application| {
            format!(
                "{}\t{}\t{}",
                application.id, application.name, application.user_entry
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(result_path, result).expect("write catalog result");
}

#[test]
fn public_catalog_uses_provider_launchability_and_xdg_precedence() {
    let fixture = FixtureDir::new();
    fixture.application(
        "user",
        "brave-browser.desktop",
        "[Desktop Entry]\nType=Application\nName=Brave Browser\nExec=/bin/true\n",
    );
    fixture.application(
        "system",
        "example.desktop",
        "[Desktop Entry]\nType=Application\nName=Example\nExec=/bin/true\n",
    );
    fixture.application(
        "system",
        "hidden.desktop",
        "[Desktop Entry]\nType=Application\nName=Hidden\nExec=/bin/true\nNoDisplay=true\n",
    );
    fixture.application(
        "system",
        "terminal.desktop",
        "[Desktop Entry]\nType=Application\nName=Terminal\nExec=/bin/true\nTerminal=true\n",
    );
    let result_path = fixture.0.join("catalog.txt");

    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "installed_catalog_helper", "--nocapture"])
        .env("AGENT_SEAT_CATALOG_RESULT", &result_path)
        .env("XDG_DATA_HOME", fixture.0.join("user"))
        .env("XDG_DATA_DIRS", fixture.0.join("system"))
        .env("XDG_CURRENT_DESKTOP", "Openbox")
        .output()
        .expect("run isolated catalog helper");
    assert!(
        output.status.success(),
        "catalog helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        fs::read_to_string(&result_path).expect("read catalog result"),
        "brave-browser.desktop\tBrave Browser\ttrue\nexample.desktop\tExample\tfalse"
    );
}

#[test]
fn catalog_refuses_relative_xdg_roots() {
    let fixture = FixtureDir::new();
    let result_path = fixture.0.join("catalog.txt");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "installed_catalog_helper", "--nocapture"])
        .env("AGENT_SEAT_CATALOG_RESULT", &result_path)
        .env("XDG_DATA_HOME", Path::new("relative"))
        .env("XDG_DATA_DIRS", fixture.0.join("system"))
        .output()
        .expect("run isolated catalog helper");

    assert!(!output.status.success());
    assert!(!result_path.exists());
}
