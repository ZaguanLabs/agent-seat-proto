//! Strict, bounded standalone provider configuration.

use std::env;
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use agent_seat_proto::{ApplicationId, BoundedList, Capability, MAX_APPLICATIONS};
use rustix::fs::{CWD, FlockOperation, Mode, OFlags, RenameFlags};
use rustix::process::geteuid;
use serde::{Deserialize, Serialize};
use toml_edit::{Array, DocumentMut, Item, Table, Value, value};

pub(crate) const MAX_CONFIG_BYTES: u64 = 64 * 1024;
/// Maximum number of capability atoms in the provider policy grant.
pub const MAX_POLICY_CAPABILITIES: usize = 10;
const DEFAULT_MAX_SESSIONS: u8 = 4;
const MAX_SESSIONS: u8 = 32;
const DEFAULT_MAX_REQUESTS: u16 = 1024;
const MAX_REQUESTS: u16 = 4096;
const DEFAULT_IO_TIMEOUT_MS: u32 = 2_000;
const MIN_IO_TIMEOUT_MS: u32 = 50;
const MAX_IO_TIMEOUT_MS: u32 = 10_000;
const TRANSACTION_ATTEMPTS: u8 = 16;

/// Minimum accepted concurrent-session policy limit.
pub const MIN_POLICY_SESSIONS: u8 = 1;
/// Maximum accepted concurrent-session policy limit.
pub const MAX_POLICY_SESSIONS: u8 = MAX_SESSIONS;
/// Minimum accepted requests-per-session policy limit.
pub const MIN_POLICY_REQUESTS: u16 = 1;
/// Maximum accepted requests-per-session policy limit.
pub const MAX_POLICY_REQUESTS: u16 = MAX_REQUESTS;
/// Minimum accepted policy I/O timeout in milliseconds.
pub const MIN_POLICY_IO_TIMEOUT_MS: u32 = MIN_IO_TIMEOUT_MS;
/// Maximum accepted policy I/O timeout in milliseconds.
pub const MAX_POLICY_IO_TIMEOUT_MS: u32 = MAX_IO_TIMEOUT_MS;

static NEXT_TRANSACTION: AtomicU64 = AtomicU64::new(1);

const FIRST_RUN_TEMPLATE_PREFIX: &str = r#"# agent-seat-x11 configuration
#
# This file was created on the first run of agent-seat-x11. The provider did
# not start. Review this policy, then change `enabled` to true when ready.
# Run `agent-seat-x11 --check-config` after every edit.
#
# The provider is a local authority: every capability below permits the MCP
# companion to observe or affect your X11 session. Keep only permissions you
# intend to grant. Unknown fields, duplicate values, and unsafe combinations
# are rejected instead of being ignored.

# Required safety switch. The generated policy remains inactive until you
# explicitly enable it.
enabled = false

# Resource limits. These defaults are suitable for an ordinary single-user
# desktop. Accepted ranges are shown next to each setting.
max_sessions = 4                 # 1..32 concurrent connections
max_requests_per_session = 1024  # 1..4096 requests per connection
io_timeout_ms = 2000             # 50..10000 milliseconds

[grant]
# Only a local socket peer whose kernel-reported UID equals this value can use
# the grant. This value was filled from the UID that created this file.
uid = "#;

const FIRST_RUN_TEMPLATE_SUFFIX: &str = r#"

# The generated policy grants only basic, title-free structure observation.
# Uncomment capabilities deliberately. `observe_titles`, `observe_events`,
# and all `manage_*` capabilities require `observe_structure`.
# `launch_execute` requires `launch_list`.
capabilities = [
  "observe_structure",
  # "observe_titles",     # Read window titles; also set titles = true below.
  # "observe_events",     # Poll bounded desktop change events.
  # "manage_activate",    # Ask the window manager to activate a client.
  # "manage_close",       # Ask a client to close politely.
  # "manage_workspace",   # Switch workspaces or move a client between them.
  # "manage_state",       # Change supported EWMH client states.
  # "manage_geometry",    # Move or resize a client frame.
  # "launch_list",        # List applications admitted by launch policy.
  # "launch_execute",     # Start an admitted desktop entry without a shell.
]

[observation]
# `none` hides every client, `current_workspace` limits visibility to the
# active workspace, and `all_workspaces` exposes clients across workspaces.
clients = "current_workspace"

# Titles are exposed only when this is true AND `observe_titles` is granted.
titles = false

[launch]
# `deny` exposes and launches nothing.
# `allow_listed` admits only desktop IDs listed in `allow`.
# `allow_installed` admits every valid discovered desktop entry except `deny`.
mode = "deny"

# Canonical desktop IDs end in `.desktop`. `allow` is consulted only in
# `allow_listed` mode but is retained in other modes so a later mode change can
# restore the selection. An ID cannot appear in both lists.
allow = []
deny = []

# User desktop entries below $XDG_DATA_HOME/applications are separately denied
# by default, even in an allowing mode. System entries remain discoverable.
allow_user_entries = false
"#;

#[derive(Clone, Debug)]
pub(crate) struct Config {
    max_sessions: usize,
    max_requests: u16,
    io_timeout: Duration,
    grant: Option<Grant>,
    observation: Observation,
    launch: LaunchPolicy,
}

/// A validated point-in-time view of an Agent Seat provider policy.
///
/// A snapshot is used as the expected original value for a later
/// [`replace_policy`] call. This prevents an editor from silently overwriting
/// changes made after it loaded the policy.
#[derive(Clone, Debug)]
pub struct PolicySnapshot {
    path: PathBuf,
    source: String,
    enabled: bool,
    device: u64,
    inode: u64,
    draft: PolicyDraft,
}

impl PolicySnapshot {
    /// Returns the absolute configuration path represented by this snapshot.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the exact validated TOML source represented by this snapshot.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Reports whether the validated policy requests provider activation.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns an independently editable typed copy of this policy.
    #[must_use]
    pub fn draft(&self) -> PolicyDraft {
        self.draft.clone()
    }
}

/// Typed, bounded provider policy suitable for a human-facing editor.
///
/// Grouped setters validate a complete prospective policy before changing the
/// draft. Rendering also passes through the provider's exact strict validator.
#[derive(Clone, Debug)]
pub struct PolicyDraft {
    raw: RawConfig,
    document: DocumentMut,
}

impl PartialEq for PolicyDraft {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl Eq for PolicyDraft {}

#[derive(Clone, Debug)]
struct Grant {
    uid: u32,
    capabilities: BoundedList<Capability, MAX_POLICY_CAPABILITIES>,
}

/// Client visibility selected by the saved observation policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientScope {
    /// Hide all clients.
    None,
    /// Expose clients on the current workspace only.
    CurrentWorkspace,
    /// Expose clients across all workspaces.
    AllWorkspaces,
}

impl Default for ClientScope {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Copy, Debug)]
struct Observation {
    clients: ClientScope,
    titles: bool,
}

/// Application admission mode selected by the saved launch policy.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    /// Deny every application.
    #[default]
    Deny,
    /// Admit only canonical desktop IDs in the allow-list.
    AllowListed,
    /// Admit every launchable installed entry except denied IDs.
    AllowInstalled,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LaunchPolicy {
    mode: LaunchMode,
    allow: BoundedList<ApplicationId, MAX_APPLICATIONS>,
    deny: BoundedList<ApplicationId, MAX_APPLICATIONS>,
    allow_user_entries: bool,
}

impl LaunchPolicy {
    pub(crate) fn allows_any(&self) -> bool {
        match self.mode {
            LaunchMode::Deny => false,
            LaunchMode::AllowListed => !self.allow.is_empty(),
            LaunchMode::AllowInstalled => true,
        }
    }

    pub(crate) fn permits(&self, application: &ApplicationId, user_entry: bool) -> bool {
        if user_entry && !self.allow_user_entries || self.deny.contains(application) {
            return false;
        }
        match self.mode {
            LaunchMode::Deny => false,
            LaunchMode::AllowListed => self.allow.contains(application),
            LaunchMode::AllowInstalled => true,
        }
    }
}

impl PolicyDraft {
    /// Reports whether this draft requests provider activation.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.raw.enabled
    }

    /// Sets the explicit provider activation switch.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.raw.enabled = enabled;
        self.document["enabled"] = value(enabled);
    }

    /// Returns `(max_sessions, max_requests_per_session, io_timeout_ms)`.
    #[must_use]
    pub const fn resource_limits(&self) -> (u8, u16, u32) {
        (
            self.raw.max_sessions,
            self.raw.max_requests_per_session,
            self.raw.io_timeout_ms,
        )
    }

    /// Replaces all resource limits as one validated edit.
    ///
    /// # Errors
    ///
    /// Returns an error when a value is outside the provider's accepted
    /// ranges. The draft is unchanged on error.
    pub fn set_resource_limits(
        &mut self,
        max_sessions: u8,
        max_requests_per_session: u16,
        io_timeout_ms: u32,
    ) -> Result<(), String> {
        let mut candidate = self.raw.clone();
        candidate.max_sessions = max_sessions;
        candidate.max_requests_per_session = max_requests_per_session;
        candidate.io_timeout_ms = io_timeout_ms;
        self.replace_if_valid(candidate)?;
        self.document["max_sessions"] = value(i64::from(max_sessions));
        self.document["max_requests_per_session"] = value(i64::from(max_requests_per_session));
        self.document["io_timeout_ms"] = value(i64::from(io_timeout_ms));
        Ok(())
    }

    /// Returns the UID attached to the grant, or `None` when no grant exists.
    #[must_use]
    pub fn grant_uid(&self) -> Option<u32> {
        self.raw.grant.as_ref().map(|grant| grant.uid)
    }

    /// Returns the granted capability atoms in saved order.
    #[must_use]
    pub fn capabilities(&self) -> &[Capability] {
        self.raw
            .grant
            .as_ref()
            .map_or(&[], |grant| grant.capabilities.as_slice())
    }

    /// Replaces the grant with capabilities for the current effective UID.
    ///
    /// This method does not add prerequisite capabilities implicitly.
    ///
    /// # Errors
    ///
    /// Returns an error for too many, duplicate, or dependency-incomplete
    /// capability atoms. The draft is unchanged on error.
    pub fn set_capabilities(&mut self, capabilities: Vec<Capability>) -> Result<(), String> {
        let capabilities = BoundedList::new(capabilities).map_err(|error| error.to_string())?;
        let mut candidate = self.raw.clone();
        candidate.grant = Some(RawGrant {
            uid: geteuid().as_raw(),
            capabilities,
        });
        self.replace_if_valid(candidate)?;
        ensure_table(&mut self.document, "grant");
        self.document["grant"]["uid"] = value(i64::from(geteuid().as_raw()));
        let mut rendered = Array::new();
        for capability in self.capabilities() {
            rendered.push(capability_name(*capability));
        }
        self.document["grant"]["capabilities"] = Item::Value(Value::Array(rendered));
        Ok(())
    }

    /// Removes the peer grant and all of its capabilities.
    pub fn clear_grant(&mut self) {
        self.raw.grant = None;
        self.document.remove("grant");
    }

    /// Returns the configured client scope and title-content switch.
    #[must_use]
    pub const fn observation(&self) -> (ClientScope, bool) {
        (self.raw.observation.clients, self.raw.observation.titles)
    }

    /// Replaces observation settings as one validated edit.
    ///
    /// # Errors
    ///
    /// Returns an error if the prospective complete policy is invalid. The
    /// draft is unchanged on error.
    pub fn set_observation(&mut self, clients: ClientScope, titles: bool) -> Result<(), String> {
        let mut candidate = self.raw.clone();
        candidate.observation = RawObservation { clients, titles };
        self.replace_if_valid(candidate)?;
        ensure_table(&mut self.document, "observation");
        self.document["observation"]["clients"] = value(client_scope_name(clients));
        self.document["observation"]["titles"] = value(titles);
        Ok(())
    }

    /// Returns the configured application admission mode.
    #[must_use]
    pub const fn launch_mode(&self) -> LaunchMode {
        self.raw.launch.mode
    }

    /// Returns canonical desktop IDs in the launch allow-list.
    #[must_use]
    pub fn launch_allow(&self) -> &[ApplicationId] {
        self.raw.launch.allow.as_slice()
    }

    /// Returns canonical desktop IDs in the launch deny-list.
    #[must_use]
    pub fn launch_deny(&self) -> &[ApplicationId] {
        self.raw.launch.deny.as_slice()
    }

    /// Reports whether user-owned desktop entries may be admitted.
    #[must_use]
    pub const fn allows_user_entries(&self) -> bool {
        self.raw.launch.allow_user_entries
    }

    /// Changes application admission mode as one valid draft edit.
    ///
    /// Allow entries, deny entries, and the user-entry gate remain unchanged.
    /// The provider consults allow entries only in allow-list mode, allowing a
    /// later mode change to restore the prior selection without data loss.
    ///
    /// # Errors
    ///
    /// Returns an error if the prospective complete policy is invalid. The
    /// draft is unchanged on error.
    pub fn set_launch_mode(&mut self, mode: LaunchMode) -> Result<(), String> {
        self.set_launch(
            mode,
            self.launch_allow().to_vec(),
            self.launch_deny().to_vec(),
            self.allows_user_entries(),
        )
    }

    /// Replaces the complete launch policy as one validated edit.
    ///
    /// # Errors
    ///
    /// Returns an error for oversized, duplicate, non-canonical, or
    /// overlapping application lists. The draft is unchanged on error.
    pub fn set_launch(
        &mut self,
        mode: LaunchMode,
        allow: Vec<ApplicationId>,
        deny: Vec<ApplicationId>,
        allow_user_entries: bool,
    ) -> Result<(), String> {
        let allow = BoundedList::new(allow).map_err(|error| error.to_string())?;
        let deny = BoundedList::new(deny).map_err(|error| error.to_string())?;
        let mut candidate = self.raw.clone();
        candidate.launch = RawLaunch {
            mode,
            allow,
            deny,
            allow_user_entries,
        };
        self.replace_if_valid(candidate)?;
        ensure_table(&mut self.document, "launch");
        self.document["launch"]["mode"] = value(launch_mode_name(mode));
        self.document["launch"]["allow"] = string_array(self.launch_allow());
        self.document["launch"]["deny"] = string_array(self.launch_deny());
        self.document["launch"]["allow_user_entries"] = value(allow_user_entries);
        Ok(())
    }

    /// Renders comment-preserving TOML accepted by the exact validator.
    ///
    /// The original snapshot remains available for a before/after comparison;
    /// rendering a draft never writes the policy.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails or the rendered policy does not
    /// pass the provider's bounded strict parser and semantic validation.
    pub fn render(&self) -> Result<String, String> {
        let source = self.document.to_string();
        validate_source(&source, geteuid().as_raw())?;
        Ok(source)
    }

    fn replace_if_valid(&mut self, candidate: RawConfig) -> Result<(), String> {
        candidate.clone().validate(geteuid().as_raw())?;
        self.raw = candidate;
        Ok(())
    }
}

fn ensure_table(document: &mut DocumentMut, name: &str) {
    document
        .entry(name)
        .or_insert_with(|| Item::Table(Table::new()));
}

fn string_array<T: std::fmt::Display>(values: &[T]) -> Item {
    let mut array = Array::new();
    for entry in values {
        array.push(entry.to_string());
    }
    Item::Value(Value::Array(array))
}

const fn client_scope_name(scope: ClientScope) -> &'static str {
    match scope {
        ClientScope::None => "none",
        ClientScope::CurrentWorkspace => "current_workspace",
        ClientScope::AllWorkspaces => "all_workspaces",
    }
}

const fn launch_mode_name(mode: LaunchMode) -> &'static str {
    match mode {
        LaunchMode::Deny => "deny",
        LaunchMode::AllowListed => "allow_listed",
        LaunchMode::AllowInstalled => "allow_installed",
    }
}

const fn capability_name(capability: Capability) -> &'static str {
    match capability {
        Capability::ObserveStructure => "observe_structure",
        Capability::ObserveTitles => "observe_titles",
        Capability::ObserveEvents => "observe_events",
        Capability::ManageActivate => "manage_activate",
        Capability::ManageClose => "manage_close",
        Capability::ManageWorkspace => "manage_workspace",
        Capability::ManageState => "manage_state",
        Capability::ManageGeometry => "manage_geometry",
        Capability::LaunchList => "launch_list",
        Capability::LaunchExecute => "launch_execute",
    }
}

impl Config {
    pub(crate) fn load(path: &Path) -> Result<(PolicySnapshot, Self), String> {
        let (snapshot, config) = Self::read(path)?;
        if !snapshot.enabled {
            return Err("provider is disabled; set enabled = true explicitly".to_owned());
        }
        Ok((snapshot, config))
    }

    pub(crate) fn check(path: &Path) -> Result<bool, String> {
        Self::read(path).map(|(snapshot, _)| snapshot.enabled)
    }

    fn read(path: &Path) -> Result<(PolicySnapshot, Self), String> {
        if !path.is_absolute() {
            return Err("configuration path must be absolute".to_owned());
        }
        let path_metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if !path_metadata.file_type().is_file() {
            return Err(format!("{} is not a regular file", path.display()));
        }
        let file =
            File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        let metadata = file
            .metadata()
            .map_err(|error| format!("cannot inspect open {}: {error}", path.display()))?;
        if metadata.dev() != path_metadata.dev() || metadata.ino() != path_metadata.ino() {
            return Err(format!("{} changed while it was opened", path.display()));
        }
        let uid = geteuid().as_raw();
        if metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
            return Err(format!(
                "{} must be owned by UID {uid} and not writable by group or others",
                path.display()
            ));
        }
        if metadata.len() > MAX_CONFIG_BYTES {
            return Err(format!(
                "{} exceeds the {MAX_CONFIG_BYTES}-byte configuration bound",
                path.display()
            ));
        }
        let capacity = usize::try_from(metadata.len().min(MAX_CONFIG_BYTES))
            .map_err(|_| "platform cannot address the bounded configuration size".to_owned())?;
        let capacity = capacity
            .checked_add(1)
            .ok_or_else(|| "platform cannot address the bounded configuration size".to_owned())?;
        let mut bytes = Vec::with_capacity(capacity);
        file.take(MAX_CONFIG_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if bytes.len() as u64 > MAX_CONFIG_BYTES {
            return Err(format!(
                "{} exceeds the {MAX_CONFIG_BYTES}-byte configuration bound",
                path.display()
            ));
        }
        let source =
            String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", path.display()))?;
        let (enabled, config, raw) = validate_source(&source, uid)
            .map_err(|error| format!("invalid {}: {error}", path.display()))?;
        let document = source.parse::<DocumentMut>().map_err(|error| {
            format!(
                "cannot preserve formatting in validated {}: {error}",
                path.display()
            )
        })?;
        Ok((
            PolicySnapshot {
                path: path.to_path_buf(),
                source,
                enabled,
                device: metadata.dev(),
                inode: metadata.ino(),
                draft: PolicyDraft { raw, document },
            },
            config,
        ))
    }

    pub(crate) const fn max_sessions(&self) -> usize {
        self.max_sessions
    }

    pub(crate) const fn max_requests(&self) -> u16 {
        self.max_requests
    }

    pub(crate) const fn io_timeout(&self) -> Duration {
        self.io_timeout
    }

    pub(crate) fn granted<'a>(
        &'a self,
        uid: u32,
        requested: impl Iterator<Item = &'a Capability>,
    ) -> Option<Vec<Capability>> {
        let grant = self.grant.as_ref().filter(|grant| grant.uid == uid)?;
        Some(
            requested
                .copied()
                .filter(|capability| grant.capabilities.contains(capability))
                .collect(),
        )
    }

    pub(crate) const fn client_scope(&self) -> ClientScope {
        self.observation.clients
    }

    pub(crate) const fn titles_enabled(&self) -> bool {
        self.observation.titles
    }

    pub(crate) const fn launch_policy(&self) -> &LaunchPolicy {
        &self.launch
    }
}

/// Reads and validates a provider policy without requiring it to be enabled.
///
/// # Errors
///
/// Returns an error when `path` is not absolute, is not a safe regular file
/// owned by the effective UID, exceeds the size bound, cannot be read, or does
/// not contain a valid strict policy.
pub fn read_policy(path: &Path) -> Result<PolicySnapshot, String> {
    Config::read(path).map(|(snapshot, _)| snapshot)
}

/// Atomically replaces a policy if it still matches `expected`.
///
/// The candidate is validated with the provider's exact parser before any
/// write. A successful replacement retains the previous policy beside the
/// target with a `.previous` suffix and returns a new validated snapshot.
///
/// # Errors
///
/// Returns an error if the candidate is invalid, the target or recovery file
/// is unsafe, another settings writer is active, the policy changed after
/// `expected` was read, or the atomic write and directory synchronization
/// cannot be completed.
pub fn replace_policy(
    expected: &PolicySnapshot,
    candidate: &str,
) -> Result<PolicySnapshot, String> {
    let uid = geteuid().as_raw();
    validate_source(candidate, uid).map_err(|error| format!("invalid candidate: {error}"))?;

    let _lock = lock_policy_directory(&expected.path, uid)?;
    let current = read_policy(&expected.path)?;
    if !same_snapshot(expected, &current) {
        return Err("configuration changed after it was read; reload before saving".to_owned());
    }
    if candidate == current.source {
        return Ok(current);
    }

    let backup_path = recovery_policy_path(&expected.path);
    let backup_exists = check_recovery_target(&backup_path, uid)?;
    let mut temporary = TemporaryPolicy::create(&expected.path, candidate)?;
    let candidate_metadata = temporary
        .file
        .metadata()
        .map_err(|error| format!("cannot inspect staged policy: {error}"))?;

    exchange(&temporary.path, &expected.path).map_err(|error| {
        format!(
            "cannot atomically install {}: {error}",
            expected.path.display()
        )
    })?;

    let displaced = read_policy(&temporary.path);
    let displaced_matches = displaced
        .as_ref()
        .is_ok_and(|snapshot| same_snapshot(expected, snapshot));
    if !displaced_matches {
        rollback_or_preserve(&mut temporary, &expected.path, &candidate_metadata)?;
        return Err(match displaced {
            Ok(_) => "configuration changed during replacement; no change was saved".to_owned(),
            Err(error) => format!(
                "configuration became unsafe during replacement; no change was saved: {error}"
            ),
        });
    }

    let backup_result = if backup_exists {
        exchange(&temporary.path, &backup_path)
    } else {
        rustix::fs::renameat_with(
            CWD,
            &temporary.path,
            CWD,
            &backup_path,
            RenameFlags::NOREPLACE,
        )
    };
    if let Err(error) = backup_result {
        rollback_or_preserve(&mut temporary, &expected.path, &candidate_metadata)?;
        return Err(format!(
            "cannot retain recovery policy {}: {error}; no change was saved",
            backup_path.display()
        ));
    }
    fs::set_permissions(&backup_path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        format!(
            "configuration was replaced but cannot secure recovery policy {}: {error}",
            backup_path.display()
        )
    })?;
    temporary.cleanup();

    sync_directory(&expected.path)?;
    read_policy(&expected.path)
}

fn same_snapshot(left: &PolicySnapshot, right: &PolicySnapshot) -> bool {
    left.device == right.device && left.inode == right.inode && left.source == right.source
}

fn suffixed_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

/// Returns the recovery-policy path paired with a provider policy path.
#[must_use]
pub fn recovery_policy_path(path: &Path) -> PathBuf {
    suffixed_path(path, ".previous")
}

struct PolicyLock(File);

impl Drop for PolicyLock {
    fn drop(&mut self) {
        let _ = rustix::fs::flock(&self.0, FlockOperation::Unlock);
    }
}

fn lock_policy_directory(path: &Path, uid: u32) -> Result<PolicyLock, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("configuration path {} has no parent", path.display()))?;
    let lock_path = parent.join(".agent-seat-config.lock");
    let descriptor = rustix::fs::open(
        &lock_path,
        OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|error| format!("cannot open settings lock {}: {error}", lock_path.display()))?;
    let file = File::from(descriptor);
    let metadata = file.metadata().map_err(|error| {
        format!(
            "cannot inspect settings lock {}: {error}",
            lock_path.display()
        )
    })?;
    if !metadata.file_type().is_file() || metadata.uid() != uid {
        return Err(format!(
            "settings lock {} must be a private regular file owned by UID {uid}",
            lock_path.display()
        ));
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| {
            format!(
                "cannot secure settings lock {}: {error}",
                lock_path.display()
            )
        })?;
    rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive).map_err(|error| {
        format!(
            "another settings writer is active for {}: {error}",
            path.display()
        )
    })?;
    Ok(PolicyLock(file))
}

fn check_recovery_target(path: &Path, uid: u32) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "cannot inspect recovery policy {}: {error}",
                path.display()
            ));
        }
    };
    if !metadata.file_type().is_file() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(format!(
            "recovery policy {} must be a private regular file owned by UID {uid}",
            path.display()
        ));
    }
    Ok(true)
}

fn exchange(left: &Path, right: &Path) -> Result<(), rustix::io::Errno> {
    rustix::fs::renameat_with(CWD, left, CWD, right, RenameFlags::EXCHANGE)
}

fn rollback_exchange(
    temporary: &Path,
    target: &Path,
    candidate_metadata: &fs::Metadata,
) -> Result<(), String> {
    let target_metadata = fs::symlink_metadata(target)
        .map_err(|error| format!("cannot inspect replacement during rollback: {error}"))?;
    if !target_metadata.file_type().is_file()
        || target_metadata.dev() != candidate_metadata.dev()
        || target_metadata.ino() != candidate_metadata.ino()
    {
        return Err(format!(
            "configuration changed again during rollback; inspect {} and {}",
            target.display(),
            temporary.display()
        ));
    }
    exchange(temporary, target).map_err(|error| {
        format!(
            "cannot roll back configuration replacement; inspect {} and {}: {error}",
            target.display(),
            temporary.display()
        )
    })
}

fn rollback_or_preserve(
    temporary: &mut TemporaryPolicy,
    target: &Path,
    candidate_metadata: &fs::Metadata,
) -> Result<(), String> {
    rollback_exchange(&temporary.path, target, candidate_metadata).inspect_err(|_| {
        temporary.preserve();
    })
}

fn sync_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("configuration path {} has no parent", path.display()))?;
    let directory = File::open(parent).map_err(|error| {
        format!(
            "cannot open configuration directory {}: {error}",
            parent.display()
        )
    })?;
    directory.sync_all().map_err(|error| {
        format!(
            "configuration was replaced but cannot synchronize directory {}: {error}",
            parent.display()
        )
    })
}

struct TemporaryPolicy {
    path: PathBuf,
    file: File,
    remove_on_drop: bool,
}

impl TemporaryPolicy {
    fn create(target: &Path, candidate: &str) -> Result<Self, String> {
        let parent = target
            .parent()
            .ok_or_else(|| format!("configuration path {} has no parent", target.display()))?;
        for _ in 0..TRANSACTION_ATTEMPTS {
            let serial = NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                ".agent-seat-config.{}.{serial}.tmp",
                std::process::id()
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true).mode(0o600);
            let mut file = match options.open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("cannot stage policy {}: {error}", path.display()));
                }
            };
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|error| {
                    format!("cannot secure staged policy {}: {error}", path.display())
                })?;
            if let Err(error) = file
                .write_all(candidate.as_bytes())
                .and_then(|()| file.sync_all())
            {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(format!(
                    "cannot write staged policy {}: {error}",
                    path.display()
                ));
            }
            return Ok(Self {
                path,
                file,
                remove_on_drop: true,
            });
        }
        Err("cannot allocate a unique bounded policy staging path".to_owned())
    }

    fn cleanup(&mut self) {
        let _ = fs::remove_file(&self.path);
    }

    fn preserve(&mut self) {
        self.remove_on_drop = false;
    }
}

impl Drop for TemporaryPolicy {
    fn drop(&mut self) {
        if self.remove_on_drop {
            self.cleanup();
        }
    }
}

/// Resolves the default provider policy path from XDG configuration variables.
///
/// # Errors
///
/// Returns an error when `XDG_CONFIG_HOME` or the `HOME` fallback is relative,
/// or when neither variable is available.
pub fn default_path() -> Result<PathBuf, String> {
    if let Some(base) = env::var_os("XDG_CONFIG_HOME") {
        let base = PathBuf::from(base);
        if !base.is_absolute() {
            return Err("XDG_CONFIG_HOME must be absolute".to_owned());
        }
        return Ok(base.join("agent-seat/config.toml"));
    }
    let home = env::var_os("HOME").ok_or_else(|| {
        "neither XDG_CONFIG_HOME nor HOME is available for configuration discovery".to_owned()
    })?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err("HOME must be absolute".to_owned());
    }
    Ok(home.join(".config/agent-seat/config.toml"))
}

/// Ensures the documented disabled policy exists at the default XDG path.
///
/// Existing policies are never modified. The returned Boolean is `true` only
/// when this call created the file.
///
/// # Errors
///
/// Returns an error when default-path discovery is unsafe or unavailable, the
/// configuration directory cannot be created, or the private policy cannot be
/// written.
pub fn ensure_default_policy() -> Result<(PathBuf, bool), String> {
    let path = default_path()?;
    let created = create_first_run_config(&path)?;
    Ok((path, created))
}

pub(crate) fn create_first_run_config(path: &Path) -> Result<bool, String> {
    let parent = path.parent().ok_or_else(|| {
        format!(
            "configuration path {} has no parent directory",
            path.display()
        )
    })?;
    let mut directory = DirBuilder::new();
    directory.recursive(true).mode(0o700);
    directory.create(parent).map_err(|error| {
        format!(
            "cannot create configuration directory {}: {error}",
            parent.display()
        )
    })?;

    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => return Err(format!("cannot create {}: {error}", path.display())),
    };
    let uid = geteuid().as_raw();
    let source = format!("{FIRST_RUN_TEMPLATE_PREFIX}{uid}{FIRST_RUN_TEMPLATE_SUFFIX}");
    if let Err(error) = file.write_all(source.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("cannot write {}: {error}", path.display()));
    }
    Ok(true)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    enabled: bool,
    #[serde(default = "default_max_sessions")]
    max_sessions: u8,
    #[serde(default = "default_max_requests")]
    max_requests_per_session: u16,
    #[serde(default = "default_io_timeout_ms")]
    io_timeout_ms: u32,
    #[serde(default)]
    grant: Option<RawGrant>,
    #[serde(default)]
    observation: RawObservation,
    #[serde(default)]
    launch: RawLaunch,
}

fn validate_source(source: &str, provider_uid: u32) -> Result<(bool, Config, RawConfig), String> {
    if source.len() as u64 > MAX_CONFIG_BYTES {
        return Err(format!(
            "candidate exceeds the {MAX_CONFIG_BYTES}-byte configuration bound"
        ));
    }
    let raw: RawConfig = toml::from_str(source).map_err(|error| error.to_string())?;
    let enabled = raw.enabled;
    let config = raw.clone().validate(provider_uid)?;
    Ok((enabled, config, raw))
}

impl RawConfig {
    fn validate(self, provider_uid: u32) -> Result<Config, String> {
        if self.max_sessions == 0 || self.max_sessions > MAX_SESSIONS {
            return Err(format!("max_sessions must be in 1..={MAX_SESSIONS}"));
        }
        if self.max_requests_per_session == 0 || self.max_requests_per_session > MAX_REQUESTS {
            return Err(format!(
                "max_requests_per_session must be in 1..={MAX_REQUESTS}"
            ));
        }
        if !(MIN_IO_TIMEOUT_MS..=MAX_IO_TIMEOUT_MS).contains(&self.io_timeout_ms) {
            return Err(format!(
                "io_timeout_ms must be in {MIN_IO_TIMEOUT_MS}..={MAX_IO_TIMEOUT_MS}"
            ));
        }
        let grant = self
            .grant
            .map(|grant| grant.validate(provider_uid))
            .transpose()?;
        Ok(Config {
            max_sessions: usize::from(self.max_sessions),
            max_requests: self.max_requests_per_session,
            io_timeout: Duration::from_millis(u64::from(self.io_timeout_ms)),
            grant,
            observation: Observation {
                clients: self.observation.clients,
                titles: self.observation.titles,
            },
            launch: self.launch.validate()?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RawGrant {
    uid: u32,
    #[serde(default)]
    capabilities: BoundedList<Capability, MAX_POLICY_CAPABILITIES>,
}

impl RawGrant {
    fn validate(self, provider_uid: u32) -> Result<Grant, String> {
        if self.uid != provider_uid {
            return Err(format!(
                "grant.uid must equal the provider's effective UID {provider_uid}"
            ));
        }
        if self
            .capabilities
            .iter()
            .enumerate()
            .any(|(index, capability)| self.capabilities[..index].contains(capability))
        {
            return Err("grant capabilities must not contain duplicates".to_owned());
        }
        if self.capabilities.contains(&Capability::ObserveTitles)
            && !self.capabilities.contains(&Capability::ObserveStructure)
        {
            return Err("observe_titles requires observe_structure".to_owned());
        }
        if self.capabilities.contains(&Capability::ObserveEvents)
            && !self.capabilities.contains(&Capability::ObserveStructure)
        {
            return Err("observe_events requires observe_structure".to_owned());
        }
        if self.capabilities.iter().any(|capability| {
            matches!(
                capability,
                Capability::ManageActivate
                    | Capability::ManageClose
                    | Capability::ManageWorkspace
                    | Capability::ManageState
                    | Capability::ManageGeometry
            )
        }) && !self.capabilities.contains(&Capability::ObserveStructure)
        {
            return Err("management capabilities require observe_structure".to_owned());
        }
        if self.capabilities.contains(&Capability::LaunchExecute)
            && !self.capabilities.contains(&Capability::LaunchList)
        {
            return Err("launch_execute requires launch_list".to_owned());
        }
        Ok(Grant {
            uid: self.uid,
            capabilities: self.capabilities,
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RawObservation {
    #[serde(default = "default_client_scope")]
    clients: ClientScope,
    #[serde(default)]
    titles: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RawLaunch {
    #[serde(default)]
    mode: LaunchMode,
    #[serde(default)]
    allow: BoundedList<ApplicationId, MAX_APPLICATIONS>,
    #[serde(default)]
    deny: BoundedList<ApplicationId, MAX_APPLICATIONS>,
    #[serde(default)]
    allow_user_entries: bool,
}

impl RawLaunch {
    fn validate(self) -> Result<LaunchPolicy, String> {
        validate_application_ids("launch.allow", &self.allow)?;
        validate_application_ids("launch.deny", &self.deny)?;
        if self
            .allow
            .iter()
            .any(|application| self.deny.contains(application))
        {
            return Err("launch.allow and launch.deny must not overlap".to_owned());
        }
        Ok(LaunchPolicy {
            mode: self.mode,
            allow: self.allow,
            deny: self.deny,
            allow_user_entries: self.allow_user_entries,
        })
    }
}

fn validate_application_ids(field: &str, applications: &[ApplicationId]) -> Result<(), String> {
    for (index, application) in applications.iter().enumerate() {
        if application.is_empty()
            || !application.ends_with(".desktop")
            || application.contains(['/', '\0'])
        {
            return Err(format!("{field} contains a non-canonical desktop ID"));
        }
        if applications[..index].contains(application) {
            return Err(format!("{field} must not contain duplicates"));
        }
    }
    Ok(())
}

const fn default_client_scope() -> ClientScope {
    ClientScope::None
}

const fn default_max_sessions() -> u8 {
    DEFAULT_MAX_SESSIONS
}

const fn default_max_requests() -> u16 {
    DEFAULT_MAX_REQUESTS
}

const fn default_io_timeout_ms() -> u32 {
    DEFAULT_IO_TIMEOUT_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path =
                env::temp_dir().join(format!("agent-seat-config-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            let mut builder = DirBuilder::new();
            builder.mode(0o700).create(&path).expect("test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn policy_lock_drop_unlocks_an_inherited_descriptor() {
        let directory = TestDirectory::new("explicit-unlock");
        let policy = directory.0.join("config.toml");
        let lock = lock_policy_directory(&policy, geteuid().as_raw()).expect("first lock");
        let inherited = lock.0.try_clone().expect("duplicate lock descriptor");

        drop(lock);

        let replacement =
            lock_policy_directory(&policy, geteuid().as_raw()).expect("lock after explicit unlock");
        drop(inherited);
        drop(replacement);
    }

    #[test]
    fn disabled_policy_is_valid_but_unknown_and_unbounded_values_are_rejected() {
        let disabled: RawConfig = toml::from_str("enabled = false").expect("parse disabled");
        assert!(disabled.validate(geteuid().as_raw()).is_ok());

        for source in [
            "enabled = false\nmax_sessions = 0",
            "enabled = true\nunknown = 1",
            "enabled = true\nmax_sessions = 0",
            "enabled = true\nmax_sessions = 33",
            "enabled = true\nio_timeout_ms = 49",
            "enabled = true\nmax_requests_per_session = 4097",
        ] {
            let accepted = toml::from_str::<RawConfig>(source)
                .is_ok_and(|raw| raw.validate(geteuid().as_raw()).is_ok());
            assert!(!accepted, "accepted {source:?}");
        }
    }

    #[test]
    fn observation_is_deny_by_default_and_strict() {
        let uid = geteuid().as_raw();
        let defaults: RawConfig = toml::from_str("enabled = true").expect("parse defaults");
        let defaults = defaults.validate(uid).expect("validate defaults");
        assert_eq!(defaults.client_scope(), ClientScope::None);
        assert!(!defaults.titles_enabled());

        let explicit: RawConfig = toml::from_str(
            "enabled = true\n[observation]\nclients = \"current_workspace\"\ntitles = true",
        )
        .expect("parse explicit observation");
        let explicit = explicit
            .validate(uid)
            .expect("validate explicit observation");
        assert_eq!(explicit.client_scope(), ClientScope::CurrentWorkspace);
        assert!(explicit.titles_enabled());

        for source in [
            "enabled = true\n[observation]\nclients = \"somewhere\"",
            "enabled = true\n[observation]\nunknown = true",
        ] {
            assert!(toml::from_str::<RawConfig>(source).is_err());
        }
    }

    #[test]
    fn dependent_capabilities_require_structure() {
        let uid = geteuid().as_raw();
        for (capability, dependency) in [
            ("observe_titles", "observe_structure"),
            ("observe_events", "observe_structure"),
            ("manage_activate", "observe_structure"),
            ("launch_execute", "launch_list"),
        ] {
            let source =
                format!("enabled = true\n[grant]\nuid = {uid}\ncapabilities = [\"{capability}\"]");
            let raw: RawConfig = toml::from_str(&source).expect("parse incomplete grant");
            let error = raw.validate(uid).expect_err("accepted incomplete grant");
            assert!(error.contains(dependency));
        }
    }

    #[test]
    fn launch_policy_is_deny_by_default_and_rejects_ambiguity() {
        let uid = geteuid().as_raw();
        let defaults: RawConfig = toml::from_str("enabled = true").expect("parse defaults");
        let defaults = defaults.validate(uid).expect("validate defaults");
        let application = ApplicationId::new("example.desktop").expect("application ID");
        assert!(!defaults.launch_policy().allows_any());
        assert!(!defaults.launch_policy().permits(&application, false));

        let inactive_list: RawConfig =
            toml::from_str("enabled = true\n[launch]\nallow = [\"example.desktop\"]")
                .expect("parse inactive allow-list");
        let inactive_list = inactive_list
            .validate(uid)
            .expect("validate inactive allow-list");
        assert!(!inactive_list.launch_policy().allows_any());
        assert!(!inactive_list.launch_policy().permits(&application, false));

        let listed: RawConfig = toml::from_str(
            "enabled = true\n[launch]\nmode = \"allow_listed\"\n\
             allow = [\"example.desktop\"]\nallow_user_entries = true",
        )
        .expect("parse listed policy");
        let listed = listed.validate(uid).expect("validate listed policy");
        assert!(listed.launch_policy().permits(&application, true));

        let installed: RawConfig = toml::from_str(
            "enabled = true\n[launch]\nmode = \"allow_installed\"\n\
             deny = [\"blocked.desktop\"]",
        )
        .expect("parse installed policy");
        let installed = installed.validate(uid).expect("validate installed policy");
        let blocked = ApplicationId::new("blocked.desktop").expect("blocked ID");
        let other = ApplicationId::new("other.desktop").expect("other ID");
        assert!(!installed.launch_policy().permits(&blocked, false));
        assert!(installed.launch_policy().permits(&other, false));
        assert!(!installed.launch_policy().permits(&other, true));

        for source in [
            "enabled = true\n[launch]\nmode = \"allow_listed\"\nallow = [\"bad/id.desktop\"]",
            "enabled = true\n[launch]\nmode = \"allow_listed\"\nallow = [\"same.desktop\"]\ndeny = [\"same.desktop\"]",
        ] {
            let accepted =
                toml::from_str::<RawConfig>(source).is_ok_and(|raw| raw.validate(uid).is_ok());
            assert!(!accepted, "accepted {source:?}");
        }
    }

    #[test]
    fn grants_match_kernel_uid_and_intersect_requests() {
        let uid = geteuid().as_raw();
        let raw: RawConfig = toml::from_str(&format!(
            "enabled = true\n[grant]\nuid = {uid}\ncapabilities = [\"observe_structure\"]"
        ))
        .expect("parse fixture");
        let config = raw.validate(uid).expect("validate fixture");
        assert_eq!(
            config.granted(
                uid,
                [Capability::ObserveStructure, Capability::ManageClose].iter()
            ),
            Some(vec![Capability::ObserveStructure])
        );
        assert_eq!(config.granted(uid + 1, [].iter()), None);
    }
}
