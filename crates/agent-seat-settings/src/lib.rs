//! Display-independent state and review model for Agent Seat Settings.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use agent_seat_proto::ApplicationDescriptor;
use agent_seat_x11::{
    PolicyDraft, PolicySnapshot, ensure_default_policy, installed_applications, read_policy,
    recovery_policy_path, replace_policy,
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
