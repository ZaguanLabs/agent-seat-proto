//! Filesystem-boundary tests for settings policy transactions.

use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use agent_seat_proto::{ApplicationId, Capability};
use agent_seat_x11::{ClientScope, LaunchMode, read_policy, replace_policy};
use rustix::process::geteuid;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-seat-policy-{label}-{}-{}",
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
        fs::write(&path, source).expect("write policy fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .expect("secure policy fixture");
        path
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn previous(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.previous", path.display()))
}

#[test]
fn valid_policy_replacement_is_atomic_private_and_recoverable() {
    let directory = FixtureDir::new("replace");
    let original = "enabled = false\n";
    let candidate = "enabled = true\nmax_sessions = 8\n";
    let path = directory.policy(original);
    let snapshot = read_policy(&path).expect("read original policy");
    assert!(!snapshot.is_enabled());

    let replaced = replace_policy(&snapshot, candidate).expect("replace policy");

    assert!(replaced.is_enabled());
    assert_eq!(replaced.source(), candidate);
    assert_eq!(
        fs::read_to_string(&path).expect("read replacement"),
        candidate
    );
    assert_eq!(
        fs::read_to_string(previous(&path)).expect("read recovery policy"),
        original
    );

    let second_candidate = "enabled = false\nmax_sessions = 6\n";
    let replaced = replace_policy(&replaced, second_candidate).expect("replace policy again");
    assert!(!replaced.is_enabled());
    assert_eq!(
        fs::read_to_string(&path).expect("read second replacement"),
        second_candidate
    );
    assert_eq!(
        fs::read_to_string(previous(&path)).expect("read rotated recovery policy"),
        candidate
    );
    for checked in [&path, &previous(&path)] {
        let mode = fs::metadata(checked)
            .expect("policy metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    let lock_mode = fs::metadata(directory.0.join(".agent-seat-config.lock"))
        .expect("settings lock metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(lock_mode, 0o600);
}

#[test]
fn typed_draft_edits_render_and_commit_through_the_exact_policy_validator() {
    let directory = FixtureDir::new("draft");
    let original = "# Keep this operator note.\nenabled = false\n";
    let path = directory.policy(original);
    let snapshot = read_policy(&path).expect("read draft source");
    let mut draft = snapshot.draft();

    assert!(!draft.is_enabled());
    assert_eq!(draft.resource_limits(), (4, 1024, 2000));
    assert_eq!(draft.grant_uid(), None);
    assert_eq!(draft.capabilities(), []);
    assert_eq!(draft.observation(), (ClientScope::None, false));
    assert_eq!(draft.launch_mode(), LaunchMode::Deny);

    let unchanged_limits = draft.resource_limits();
    assert!(draft.set_resource_limits(0, 1024, 2000).is_err());
    assert_eq!(draft.resource_limits(), unchanged_limits);
    assert!(
        draft
            .set_capabilities(vec![Capability::LaunchExecute])
            .is_err()
    );
    assert_eq!(draft.capabilities(), []);

    draft.set_enabled(true);
    draft
        .set_resource_limits(8, 2048, 750)
        .expect("valid resource edit");
    draft
        .set_capabilities(vec![
            Capability::ObserveStructure,
            Capability::ObserveTitles,
            Capability::LaunchList,
            Capability::LaunchExecute,
        ])
        .expect("complete capability edit");
    draft
        .set_observation(ClientScope::CurrentWorkspace, true)
        .expect("valid observation edit");
    let brave = ApplicationId::new("brave-browser.desktop").expect("Brave desktop ID");
    draft
        .set_launch(
            LaunchMode::AllowListed,
            vec![brave.clone()],
            Vec::new(),
            false,
        )
        .expect("valid launch edit");
    assert!(
        draft
            .set_launch(
                LaunchMode::AllowListed,
                vec![brave.clone(), brave],
                Vec::new(),
                false,
            )
            .is_err()
    );
    assert_eq!(draft.launch_allow().len(), 1);

    let rendered = draft.render().expect("render validated policy");
    assert!(rendered.starts_with("# Keep this operator note."));
    let replaced = replace_policy(&snapshot, &rendered).expect("commit rendered policy");
    let saved = replaced.draft();
    assert!(saved.is_enabled());
    assert_eq!(saved.resource_limits(), (8, 2048, 750));
    assert_eq!(saved.observation(), (ClientScope::CurrentWorkspace, true));
    assert_eq!(saved.launch_mode(), LaunchMode::AllowListed);
    assert_eq!(saved.launch_allow().len(), 1);
    assert_eq!(
        fs::read_to_string(previous(&path)).expect("read draft recovery policy"),
        original
    );
}

#[test]
fn launch_mode_transitions_preserve_inactive_allow_entries() {
    let directory = FixtureDir::new("launch-mode-transition");
    let source = "enabled = false\n\
                  [launch]\n\
                  mode = \"allow_listed\"\n\
                  allow = [\"brave-browser.desktop\"]\n\
                  deny = [\"blocked.desktop\"]\n\
                  allow_user_entries = true\n";
    let path = directory.policy(source);
    let snapshot = read_policy(&path).expect("read transition policy");
    let mut draft = snapshot.draft();
    let brave = ApplicationId::new("brave-browser.desktop").expect("Brave desktop ID");
    let blocked = ApplicationId::new("blocked.desktop").expect("blocked desktop ID");

    draft
        .set_launch_mode(LaunchMode::AllowInstalled)
        .expect("switch to installed mode");
    assert_eq!(draft.launch_mode(), LaunchMode::AllowInstalled);
    assert_eq!(draft.launch_allow(), std::slice::from_ref(&brave));
    assert_eq!(draft.launch_deny(), std::slice::from_ref(&blocked));
    assert!(draft.allows_user_entries());

    let installed_source = draft.render().expect("render installed mode");
    let installed = replace_policy(&snapshot, &installed_source).expect("save installed mode");
    assert_eq!(
        installed.draft().launch_allow(),
        std::slice::from_ref(&brave)
    );

    draft
        .set_launch_mode(LaunchMode::Deny)
        .expect("switch to deny mode");
    assert_eq!(draft.launch_mode(), LaunchMode::Deny);
    assert_eq!(draft.launch_deny(), std::slice::from_ref(&blocked));

    draft
        .set_launch_mode(LaunchMode::AllowListed)
        .expect("switch back to listed mode");
    assert_eq!(draft.launch_mode(), LaunchMode::AllowListed);
    assert_eq!(draft.launch_allow(), std::slice::from_ref(&brave));
    assert_eq!(draft.launch_deny(), std::slice::from_ref(&blocked));
}

#[test]
fn settings_edits_preserve_the_private_device_profile() {
    let directory = FixtureDir::new("private-device-profile");
    let source = "enabled = false\n\
                  [input]\n\
                  broker_socket = \"/run/agent-seat-activity/provider.sock\"\n\
                  broker_peer_uid = 0\n\
                  provider_private_devices = true\n";
    let path = directory.policy(source);
    let snapshot = read_policy(&path).expect("read private-device policy");
    let mut draft = snapshot.draft();
    draft
        .set_launch_mode(LaunchMode::AllowInstalled)
        .expect("edit unrelated launch mode");

    let rendered = draft.render().expect("render private-device policy");
    assert!(rendered.contains("provider_private_devices = true"));
    assert!(rendered.contains("broker_socket = \"/run/agent-seat-activity/provider.sock\""));
    replace_policy(&snapshot, &rendered).expect("save private-device policy");
}

#[test]
fn typed_draft_safely_edits_valid_inline_tables() {
    let directory = FixtureDir::new("inline-draft");
    let uid = geteuid().as_raw();
    let source = format!(
        "enabled = false\n\
         grant = {{ uid = {uid}, capabilities = [] }}\n\
         observation = {{ clients = \"none\", titles = false }}\n\
         launch = {{ mode = \"deny\", allow = [], deny = [], allow_user_entries = false }}\n"
    );
    let path = directory.policy(&source);
    let snapshot = read_policy(&path).expect("read inline policy");
    let mut draft = snapshot.draft();

    draft
        .set_capabilities(vec![Capability::ObserveStructure])
        .expect("edit inline grant");
    draft
        .set_observation(ClientScope::AllWorkspaces, false)
        .expect("edit inline observation");
    draft
        .set_launch(LaunchMode::AllowInstalled, Vec::new(), Vec::new(), false)
        .expect("edit inline launch");

    let rendered = draft.render().expect("render edited inline tables");
    replace_policy(&snapshot, &rendered).expect("commit edited inline tables");
}

#[test]
fn settings_lock_failure_leaves_policy_unchanged() {
    let directory = FixtureDir::new("lock-failure");
    let original = "enabled = false\n";
    let path = directory.policy(original);
    let snapshot = read_policy(&path).expect("read original policy");
    fs::create_dir(directory.0.join(".agent-seat-config.lock")).expect("blocking lock directory");

    let error =
        replace_policy(&snapshot, "enabled = true\n").expect_err("accepted unusable settings lock");

    assert!(error.contains("settings lock"));
    assert_eq!(
        fs::read_to_string(&path).expect("read unchanged policy"),
        original
    );
    assert!(!previous(&path).exists());
}

#[test]
fn invalid_candidate_and_stale_snapshot_leave_current_policy_unchanged() {
    let directory = FixtureDir::new("refuse");
    let original = "enabled = false\n";
    let externally_changed = "enabled = true\nmax_sessions = 3\n";
    let path = directory.policy(original);
    let snapshot = read_policy(&path).expect("read original policy");

    let invalid = replace_policy(&snapshot, "enabled = false\nmax_sessions = 0\n")
        .expect_err("accepted invalid candidate");
    assert!(invalid.contains("max_sessions"));
    assert_eq!(
        fs::read_to_string(&path).expect("read unchanged policy"),
        original
    );

    fs::write(&path, externally_changed).expect("external policy edit");
    let stale = replace_policy(&snapshot, "enabled = true\n").expect_err("accepted stale snapshot");
    assert!(stale.contains("changed after it was read"));
    assert_eq!(
        fs::read_to_string(&path).expect("read external policy"),
        externally_changed
    );
    assert!(!previous(&path).exists());
}

#[test]
fn concurrent_replacements_do_not_overwrite_each_other() {
    let directory = FixtureDir::new("concurrent");
    let original = "enabled = false\n";
    let first = "enabled = true\nmax_sessions = 2\n";
    let second = "enabled = true\nmax_sessions = 3\n";
    let path = directory.policy(original);
    let snapshot = read_policy(&path).expect("read shared policy");
    let barrier = Arc::new(Barrier::new(3));

    let handles = [first, second].map(|candidate| {
        let snapshot = snapshot.clone();
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            replace_policy(&snapshot, candidate)
        })
    });
    barrier.wait();
    let results = handles.map(|handle| handle.join().expect("settings writer thread"));

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let saved = fs::read_to_string(&path).expect("read winning policy");
    assert!(saved == first || saved == second);
    assert_eq!(
        fs::read_to_string(previous(&path)).expect("read recovery policy"),
        original
    );
}

#[test]
fn symlink_nonregular_and_unsafe_recovery_targets_are_refused() {
    let directory = FixtureDir::new("unsafe");
    let victim = directory.policy("enabled = false\n");
    let symlink_path = directory.0.join("linked.toml");
    symlink(&victim, &symlink_path).expect("policy symlink");
    assert!(
        read_policy(&symlink_path)
            .expect_err("accepted symlink")
            .contains("regular file")
    );

    let directory_target = directory.0.join("directory.toml");
    fs::create_dir(&directory_target).expect("directory target");
    assert!(
        read_policy(&directory_target)
            .expect_err("accepted directory")
            .contains("regular file")
    );

    let snapshot = read_policy(&victim).expect("read safe target");
    let recovery = previous(&victim);
    symlink(&symlink_path, &recovery).expect("unsafe recovery symlink");
    let error =
        replace_policy(&snapshot, "enabled = true\n").expect_err("accepted unsafe recovery target");
    assert!(error.contains("recovery policy"));
    assert_eq!(
        fs::read_to_string(&victim).expect("read target after refusal"),
        "enabled = false\n"
    );
}
