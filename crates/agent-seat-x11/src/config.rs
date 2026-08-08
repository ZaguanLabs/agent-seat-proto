//! Strict, bounded standalone provider configuration.

use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use agent_seat_proto::{ApplicationId, BoundedList, Capability, MAX_APPLICATIONS};
use rustix::process::geteuid;
use serde::Deserialize;

const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_CAPABILITIES: usize = 10;
const DEFAULT_MAX_SESSIONS: u8 = 4;
const MAX_SESSIONS: u8 = 32;
const DEFAULT_MAX_REQUESTS: u16 = 1024;
const MAX_REQUESTS: u16 = 4096;
const DEFAULT_IO_TIMEOUT_MS: u32 = 2_000;
const MIN_IO_TIMEOUT_MS: u32 = 50;
const MAX_IO_TIMEOUT_MS: u32 = 10_000;

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

# Canonical desktop IDs end in `.desktop`. `allow` must stay empty unless mode
# is `allow_listed`; an ID cannot appear in both lists.
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

#[derive(Clone, Debug)]
struct Grant {
    uid: u32,
    capabilities: BoundedList<Capability, MAX_CAPABILITIES>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClientScope {
    None,
    CurrentWorkspace,
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LaunchMode {
    #[default]
    Deny,
    AllowListed,
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

impl Config {
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
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
            std::str::from_utf8(&bytes).map_err(|_| format!("{} is not UTF-8", path.display()))?;
        let raw: RawConfig = toml::from_str(source)
            .map_err(|error| format!("invalid {}: {error}", path.display()))?;
        raw.validate(uid)
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

pub(crate) fn default_path() -> Result<PathBuf, String> {
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

#[derive(Deserialize)]
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

impl RawConfig {
    fn validate(self, provider_uid: u32) -> Result<Config, String> {
        if !self.enabled {
            return Err("provider is disabled; set enabled = true explicitly".to_owned());
        }
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGrant {
    uid: u32,
    #[serde(default)]
    capabilities: BoundedList<Capability, MAX_CAPABILITIES>,
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

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawObservation {
    #[serde(default = "default_client_scope")]
    clients: ClientScope,
    #[serde(default)]
    titles: bool,
}

#[derive(Default, Deserialize)]
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
        if self.mode != LaunchMode::AllowListed && !self.allow.is_empty() {
            return Err("launch.allow is valid only in allow_listed mode".to_owned());
        }
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

    #[test]
    fn disabled_unknown_and_unbounded_values_are_rejected() {
        for source in [
            "enabled = false",
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
            "enabled = true\n[launch]\nallow = [\"example.desktop\"]",
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
