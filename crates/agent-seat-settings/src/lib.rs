//! Display-independent state and review model for Agent Seat Settings.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use agent_seat_proto::ApplicationDescriptor;
use agent_seat_x11::{
    ActivePolicyStatus, PolicyDraft, PolicySnapshot, active_policy_status, ensure_default_policy,
    installed_applications, read_policy, recovery_policy_path, replace_policy,
};

/// One loaded saved policy and an independently editable draft.
#[derive(Clone, Debug)]
pub struct SettingsModel {
    snapshot: PolicySnapshot,
    original: PolicyDraft,
    draft: PolicyDraft,
}

impl SettingsModel {
    /// Opens an existing explicit provider policy.
    ///
    /// # Errors
    ///
    /// Returns the provider's strict read and validation error.
    pub fn open(path: &Path) -> Result<Self, String> {
        Ok(Self::from_snapshot(read_policy(path)?))
    }

    /// Opens the default policy, creating the documented disabled template
    /// when it does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error when XDG discovery, safe creation, reading, or strict
    /// provider validation fails.
    pub fn open_default() -> Result<(Self, bool), String> {
        let (path, created) = ensure_default_policy()?;
        Ok((Self::open(&path)?, created))
    }

    fn from_snapshot(snapshot: PolicySnapshot) -> Self {
        let draft = snapshot.draft();
        Self {
            snapshot,
            original: draft.clone(),
            draft,
        }
    }

    /// Returns the absolute saved-policy path.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.snapshot.path()
    }

    /// Returns the exact saved source used as the conflict-detection snapshot.
    #[must_use]
    pub fn saved_source(&self) -> &str {
        self.snapshot.source()
    }

    /// Reports whether the last loaded saved policy requests activation.
    #[must_use]
    pub fn saved_enabled(&self) -> bool {
        self.original.is_enabled()
    }

    /// Returns the editable typed policy.
    #[must_use]
    pub const fn draft(&self) -> &PolicyDraft {
        &self.draft
    }

    /// Applies one edit to a clone and commits it to the draft only on success.
    ///
    /// # Errors
    ///
    /// Returns the edit's exact validation error. The current draft is
    /// unchanged on error.
    pub fn edit(
        &mut self,
        operation: impl FnOnce(&mut PolicyDraft) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut candidate = self.draft.clone();
        operation(&mut candidate)?;
        self.draft = candidate;
        Ok(())
    }

    /// Restores the draft to the last loaded or saved policy values.
    pub fn discard_draft(&mut self) {
        self.draft = self.original.clone();
    }

    /// Reports whether typed policy values differ from the loaded policy.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        self.draft != self.original
    }

    /// Returns the exact source proposed for saving.
    ///
    /// An unchanged draft returns the original source byte-for-byte.
    ///
    /// # Errors
    ///
    /// Returns an exact provider validation or rendering error.
    pub fn candidate_source(&self) -> Result<String, String> {
        if self.has_changes() {
            self.draft.render()
        } else {
            Ok(self.snapshot.source().to_owned())
        }
    }

    /// Returns a bounded unified before/after view of the changed source.
    ///
    /// Unchanged context is reduced to two lines around one conservative
    /// changed region. Removed and added lines retain their exact text after
    /// the leading diff marker.
    ///
    /// # Errors
    ///
    /// Returns an exact provider rendering or validation error.
    pub fn unified_diff(&self) -> Result<String, String> {
        if !self.has_changes() {
            return Ok("No policy changes.\n".to_owned());
        }
        let after = self.candidate_source()?;
        Ok(unified_diff(self.snapshot.source(), &after))
    }

    /// Saves a changed draft through the provider's atomic transaction API.
    ///
    /// # Errors
    ///
    /// Returns an error when there are no changes, rendering fails, the saved
    /// policy became stale, or atomic replacement fails.
    pub fn save(&mut self) -> Result<(), String> {
        if !self.has_changes() {
            return Err("there are no policy changes to save".to_owned());
        }
        let candidate = self.candidate_source()?;
        let snapshot = replace_policy(&self.snapshot, &candidate)?;
        *self = Self::from_snapshot(snapshot);
        Ok(())
    }

    /// Reloads the current path, discarding the in-memory draft.
    ///
    /// # Errors
    ///
    /// Returns the provider's strict read and validation error.
    pub fn reload(&mut self) -> Result<(), String> {
        *self = Self::open(self.path())?;
        Ok(())
    }

    /// Replaces the current policy with its validated `.previous` recovery
    /// policy and keeps the displaced current policy as the new recovery file.
    ///
    /// # Errors
    ///
    /// Returns an error when the recovery policy is missing or unsafe, either
    /// policy is invalid, the current snapshot became stale, or replacement
    /// fails.
    pub fn restore_previous(&mut self) -> Result<(), String> {
        let recovery = read_policy(&recovery_policy_path(self.path()))?;
        let snapshot = replace_policy(&self.snapshot, recovery.source())?;
        *self = Self::from_snapshot(snapshot);
        Ok(())
    }

    /// Returns the paired recovery-policy path.
    #[must_use]
    pub fn recovery_path(&self) -> PathBuf {
        recovery_policy_path(self.path())
    }

    /// Reads lock-held evidence of the policy loaded by running providers.
    ///
    /// This does not connect to X11 or a provider socket and does not grant
    /// authority. Absence is reported distinctly because an older provider
    /// may not publish evidence.
    ///
    /// # Errors
    ///
    /// Returns the provider library's runtime-directory or marker safety error.
    pub fn active_policy_status(&self) -> Result<ActivePolicyStatus, String> {
        active_policy_status(&self.snapshot)
    }

    /// Discovers the same bounded launchable application catalog as the
    /// provider, without applying the saved launch policy.
    ///
    /// # Errors
    ///
    /// Returns the provider catalog's XDG safety or resource-bound error.
    pub fn application_catalog(&self) -> Result<Vec<ApplicationDescriptor>, String> {
        installed_applications()
    }
}

fn unified_diff(before: &str, after: &str) -> String {
    let before = before.lines().collect::<Vec<_>>();
    let after = after.lines().collect::<Vec<_>>();
    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(left, right)| left == right)
        .count();
    let possible_suffix = before.len().min(after.len()).saturating_sub(prefix);
    let suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(possible_suffix)
        .take_while(|(left, right)| left == right)
        .count();
    let before_changed_end = before.len().saturating_sub(suffix);
    let after_changed_end = after.len().saturating_sub(suffix);
    let context_start = prefix.saturating_sub(2);
    let context_end_before = (before_changed_end + 2).min(before.len());
    let mut result = String::from("--- saved policy\n+++ draft policy\n");
    for line in &before[context_start..prefix] {
        result.push(' ');
        result.push_str(line);
        result.push('\n');
    }
    for line in &before[prefix..before_changed_end] {
        result.push('-');
        result.push_str(line);
        result.push('\n');
    }
    for line in &after[prefix..after_changed_end] {
        result.push('+');
        result.push_str(line);
        result.push('\n');
    }
    for line in &before[before_changed_end..context_end_before] {
        result.push(' ');
        result.push_str(line);
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::unified_diff;

    #[test]
    fn diff_keeps_local_context_and_exact_changed_lines() {
        let before = "one\ntwo\nthree\nfour\nfive\n";
        let after = "one\ntwo\nchanged\nfour\nfive\n";
        assert_eq!(
            unified_diff(before, after),
            "--- saved policy\n+++ draft policy\n one\n two\n-three\n+changed\n four\n five\n"
        );
    }
}
