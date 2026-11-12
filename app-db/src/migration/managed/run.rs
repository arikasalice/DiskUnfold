//! Batch migration orchestration with one terminal backup diagnostic.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use super::{
    Definitions, MigrationError, RetentionDiagnostic, apply_managed_step, retention_or_unavailable,
};
use crate::migration::{applied_count, validate_definitions};

/// Outcome of a complete managed migration run and its terminal backup scan.
///
/// The scan observes the managed directory once after the final step. Each
/// step still publishes and verifies its own backup before applying its
/// definition.
#[derive(Debug)]
pub struct ManagedRunOutcome {
    /// Last committed revision, or the starting revision when no step ran.
    pub revision: u32,
    /// Number of definitions committed during this run.
    pub applied_steps: usize,
    /// Diagnostic of the backup directory at the end of this run.
    pub retention: RetentionDiagnostic,
}

/// Migration failure together with the terminal backup diagnostic.
///
/// Earlier successful steps remain committed. The failed step remains
/// unapplied, and its already-published backup is included in the terminal
/// diagnostic when it can be inspected.
#[derive(Debug)]
pub struct ManagedRunFailure {
    /// Failure from backup publication or applying a definition.
    pub migration: MigrationError,
    /// Diagnostic collected after the failure, or an unavailable diagnostic.
    pub retention: RetentionDiagnostic,
}

/// Applies every definition after `starting_revision` and scans retention once.
///
/// The caller must hold the GUI instance lock and prevent replacement of the
/// database and backup directory. Every applied step publishes a complete
/// backup before its transaction and commits independently. If a later step
/// fails, earlier commits and the failed step's backup remain in place; the
/// caller can reopen the database, read its current revision, and resume.
/// The starting revision must match the revision observed by each step; a
/// mismatch fails before that step publishes a backup. With no later
/// definitions, the current revision is checked read-only and no backup is
/// published.
///
/// The terminal scan reads the current directory contents without reusing
/// prior validation results. Changes completed before the scan are therefore
/// evaluated by the existing integrity and inspection-failure rules. Scan
/// failure is returned as an unavailable retention diagnostic while
/// preserving the migration result.
pub fn apply_managed_run(
    path: &Path,
    root: &Path,
    definitions: &Definitions<'_>,
    starting_revision: u32,
) -> Result<ManagedRunOutcome, ManagedRunFailure> {
    apply_managed_run_with(
        path,
        root,
        definitions,
        starting_revision,
        super::inspect_retention,
    )
}

pub(super) fn apply_managed_run_with(
    path: &Path,
    root: &Path,
    definitions: &Definitions<'_>,
    starting_revision: u32,
    inspect: impl FnOnce(&Path) -> Result<RetentionDiagnostic, MigrationError>,
) -> Result<ManagedRunOutcome, ManagedRunFailure> {
    let mut revision = starting_revision;
    let mut applied_steps = 0;
    let mut failure = None;
    let expected_steps = definitions
        .steps
        .iter()
        .filter(|definition| definition.number > starting_revision);
    let has_steps = definitions
        .steps
        .iter()
        .any(|definition| definition.number > starting_revision);
    if !has_steps {
        failure = validate_definitions(definitions)
            .and_then(|()| applied_count(definitions, starting_revision).map(|_| ()))
            .and_then(|()| {
                let current = current_revision(path)?;
                if current == starting_revision {
                    Ok(())
                } else {
                    Err(MigrationError::VersionMismatch)
                }
            })
            .err();
    }
    for definition in expected_steps {
        if failure.is_some() {
            break;
        }
        match apply_managed_step(path, root, definitions, Some(revision)) {
            Ok(step) if step.revision == definition.number => {
                revision = step.revision;
                applied_steps += 1;
            }
            Ok(_) => {
                failure = Some(MigrationError::VersionMismatch);
                break;
            }
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }

    let retention = retention_or_unavailable(inspect(root));
    match failure {
        Some(migration) => Err(ManagedRunFailure {
            migration,
            retention,
        }),
        None => Ok(ManagedRunOutcome {
            revision,
            applied_steps,
            retention,
        }),
    }
}

fn current_revision(path: &Path) -> Result<u32, MigrationError> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(db.pragma_query_value(None, "user_version", |row| row.get(0))?)
}
