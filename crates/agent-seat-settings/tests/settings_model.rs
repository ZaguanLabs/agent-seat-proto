//! Display-independent settings model and command process tests.

use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_seat_settings::SettingsModel;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-seat-settings-{label}-{}-{}",
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

    fn policy(&self, source: &str) -> PathBuf {
        let path = self.0.join("config.toml");
        fs::write(&path, source).expect("write policy");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("secure policy");
        path
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn settings(arguments: &[&str], policy: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_agent-seat-settings"))
        .args(["--config", policy.to_str().expect("policy UTF-8")])
        .args(arguments)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("run settings command")
}

#[test]
fn model_tracks_typed_changes_and_preserves_comments_across_save_and_restore() {
    let directory = FixtureDir::new("model");
    let original = "# Operator context stays here.\nenabled = false\n";
    let path = directory.policy(original);
    let mut model = SettingsModel::open(&path).expect("open policy");

    assert!(!model.has_changes());
    assert_eq!(
        model.candidate_source().expect("unchanged source"),
        original
    );
    model
        .edit(|draft| {
            draft.set_enabled(true);
            Ok(())
        })
        .expect("edit activation");
    assert!(model.has_changes());
    assert!(
        model
            .candidate_source()
            .expect("candidate source")
            .starts_with("# Operator context stays here.")
    );
    model.save().expect("save policy");
    assert!(!model.has_changes());
    assert!(model.draft().is_enabled());

    model.restore_previous().expect("restore previous policy");
    assert!(!model.draft().is_enabled());
    assert_eq!(model.saved_source(), original);
    assert!(
        fs::read_to_string(model.recovery_path())
            .expect("read displaced recovery")
            .contains("enabled = true")
    );
}

#[test]
fn display_independent_commands_check_print_restore_and_reject_conflicts() {
    let directory = FixtureDir::new("commands");
    let original = "enabled = false\n";
    let path = directory.policy(original);
    let mut model = SettingsModel::open(&path).expect("open command fixture");
    model
        .edit(|draft| {
            draft.set_enabled(true);
            Ok(())
        })
        .expect("edit command fixture");
    model.save().expect("seed recovery policy");

    let checked = settings(&["--check"], &path);
    assert!(checked.status.success());
    assert!(String::from_utf8_lossy(&checked.stdout).contains("valid and enabled"));

    let printed = settings(&["--print"], &path);
    assert!(printed.status.success());
    assert!(String::from_utf8_lossy(&printed.stdout).contains("enabled = true"));

    let conflict = settings(&["--check", "--print"], &path);
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("cannot be combined"));

    let restored = settings(&["--restore-previous"], &path);
    assert!(restored.status.success());
    assert_eq!(
        fs::read_to_string(&path).expect("restored source"),
        original
    );
    assert!(String::from_utf8_lossy(&restored.stdout).contains("Restart a running provider"));
}

#[test]
fn help_is_complete_without_a_display() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-seat-settings"))
        .arg("--help")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("run settings help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    for expected in [
        "--config PATH",
        "--check",
        "--print",
        "--restore-previous",
        "do not initialize GTK or X11",
        "restarted",
    ] {
        assert!(stdout.contains(expected), "missing {expected:?}");
    }
}
