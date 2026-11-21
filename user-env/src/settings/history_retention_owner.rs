use du_core_contracts::settings::{
    SettingsChanges, SettingsCommit, SettingsSource, SettingsStore, SettingsStoreError,
};

use super::{Changed, SettingsError, SettingsService};

impl<S: SettingsStore> SettingsService<S> {
    /// Saves the hidden F21 retention values through the owner-only settings path.
    ///
    /// `enabled` requires a specified age. The method validates the age, commits
    /// both retention keys with `SettingsSource::Owner`, and updates this service
    /// snapshot only after the store confirms the commit.
    pub fn save_history_retention_policy(
        &mut self,
        enabled: bool,
        specified_days: Option<u32>,
        timestamp_ms: i64,
    ) -> Result<Changed, SettingsError> {
        if self.restore_status.write_locked {
            return Err(SettingsError::RestoreLocked);
        }
        if enabled && specified_days.is_none() {
            return Err(SettingsError::InvalidValue);
        }
        if let Some(days) = specified_days {
            crate::history_retention::validate_days(&serde_json::json!(days))?;
        }

        let changes = SettingsChanges::from([
            (
                crate::history_retention::ENABLED_KEY.to_owned(),
                enabled.then_some(serde_json::Value::Bool(true)),
            ),
            (
                crate::history_retention::DAYS_KEY.to_owned(),
                specified_days.map(|days| serde_json::json!(days)),
            ),
        ]);
        let next = self
            .store
            .commit(
                self.snapshot.revision,
                &changes,
                SettingsCommit {
                    source: SettingsSource::Owner,
                    timestamp_ms,
                },
            )
            .map_err(|error| match error {
                SettingsStoreError::Conflict => SettingsError::StaleVersion,
                other => SettingsError::Store(other),
            })?;
        for (key, value) in &changes {
            match value {
                Some(value) => {
                    self.snapshot.values.insert(key.clone(), value.clone());
                }
                None => {
                    self.snapshot.values.remove(key);
                }
            }
        }
        self.snapshot.revision = next;
        Ok(Changed {
            revision: next,
            keys: changes.keys().cloned().collect(),
        })
    }
}
