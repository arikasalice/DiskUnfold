//! The built-in settings, job history and recent scope schema chain.

use std::path::Path;

use crate::{
    AppDb, OpenError, SCHEMA_VERSION, create_gui_with, initialize_settings,
    migration::{self, Definition, Definitions, MigrationError},
};

const TERMINAL_SUMMARY: &str = concat!(
    "CREATE TABLE app_db_purge_gate (
        gate_key INTEGER PRIMARY KEY CHECK(gate_key = 1),
        open INTEGER NOT NULL DEFAULT 0 CHECK(open IN (0, 1))
    ) STRICT;
    INSERT INTO app_db_purge_gate (gate_key) VALUES (1);\n",
    include_str!("job_history/terminal_summary.sql")
);

/// Current database version for the desktop store with job history.
pub const HISTORY_SCHEMA_VERSION: u32 = 5;

/// Desktop migrations in application-wide order, after the known settings schema.
pub const HISTORY_DEFINITIONS: Definitions<'static> = Definitions {
    baseline: SCHEMA_VERSION,
    steps: &[
        Definition {
            number: 2,
            sql: TERMINAL_SUMMARY,
        },
        Definition {
            number: 3,
            sql: include_str!("job_history/history_records.sql"),
        },
        Definition {
            number: 4,
            sql: crate::recent_scope_schema::DEFINITION,
        },
        Definition {
            number: HISTORY_SCHEMA_VERSION,
            sql: include_str!("ruleset_schema.sql"),
        },
    ],
};

impl AppDb {
    /// Creates a complete history-enabled database before publishing it.
    /// An existing file is never overwritten.
    pub fn create_gui_with_history(path: &Path) -> Result<Self, OpenError> {
        create_gui_with(
            path,
            |tx| {
                initialize_settings(tx)?;
                migration::initialize(tx, &HISTORY_DEFINITIONS).map_err(schema_error)
            },
            || Ok(()),
            Self::open_gui_with_history,
        )
    }

    /// Opens a history-enabled database after checking its migration records.
    /// The caller migrates an older database under the GUI instance lock.
    pub fn open_gui_with_history(path: &Path) -> Result<Self, OpenError> {
        Self::open_gui_checked(path, |connection| {
            migration::verify_registered(connection, &HISTORY_DEFINITIONS, true)
                .map_err(schema_error)
        })
    }
}

pub(crate) fn schema_error(error: MigrationError) -> OpenError {
    OpenError::HistorySchema(Box::new(error))
}

#[cfg(test)]
mod tests;
