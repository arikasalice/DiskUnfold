//! Stores scan-completion observations through the GUI's shared database writer.

use du_core_contracts::scope_observation_log::{
    ScopeObservation, ScopeObservationAppend, ScopeObservationLog, ScopeObservationWriteError,
};

use crate::shared_database::SharedDatabase;

/// The F03 observation log on the connection that also writes settings and job history.
///
/// Each append is its own transaction inside app-db; this adapter only takes the
/// connection and maps nothing else, so the caller sees app-db's failure classes.
pub(crate) struct SharedScopeObservations {
    database: SharedDatabase,
}

impl SharedDatabase {
    /// An observation log writing through this shared connection.
    pub(crate) fn scope_observations(&self) -> SharedScopeObservations {
        SharedScopeObservations {
            database: self.clone(),
        }
    }
}

impl ScopeObservationLog for SharedScopeObservations {
    fn append(
        &self,
        observation: &ScopeObservation,
    ) -> Result<ScopeObservationAppend, ScopeObservationWriteError> {
        self.database
            .0
            .connection
            .lock()
            .map_err(|_| ScopeObservationWriteError::Storage)?
            .append_scope_observation(observation)
    }
}

#[cfg(test)]
mod tests;
