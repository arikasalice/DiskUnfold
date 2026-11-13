//! Batch migration orchestration with one terminal backup diagnostic.
//!
//! F14 DLD rev79 §2.5.5 keeps completed steps after a failure; the user decided to follow F15
//! DLD rev70 §2.6 (FR8.HD1.app-db.DD1) instead, which restores the pre-migration backup. See
//! the restore module documentation and docs/handoff for the design revision note.

use std::path::Path;

use super::{
    Definitions, JournalPlan, MigrationError, RestoreStage, RestoreStatus, RetentionDiagnostic,
    apply_managed_step, restore, retention_or_unavailable,
};
use crate::migration;

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

/// Migration failure together with the restore result and the terminal backup diagnostic.
///
/// When the run had published its pre-migration backup, the live database was restored to that
/// backup as described by [RestoreStatus]. Every published backup stays in place.
#[derive(Debug)]
pub struct ManagedRunFailure {
    /// Failure from backup publication or applying a definition.
    pub migration: MigrationError,
    /// Whether the pre-migration backup was restored over the live database.
    pub restore: RestoreStatus,
    /// Diagnostic collected after the failure, or an unavailable diagnostic.
    pub retention: RetentionDiagnostic,
}

/// Applies every definition after the starting revision and scans retention once.
///
/// The caller must hold the GUI instance lock, prevent replacement of the
/// database and backup directory, and have called
/// [recover_interrupted](super::recover_interrupted) first; a leftover journal fails the
/// run with [MigrationError::Interrupted] before anything changes.
///
/// Every step publishes a complete backup before its transaction and commits independently.
/// The first step additionally verifies its backup and writes the journal before the first
/// write. If any step fails afterwards, the transaction is rolled back, every handle is closed
/// and the verified pre-migration backup is restored over the live database, which is then
/// reopened at the old revision and integrity checked. Earlier committed steps are discarded.
/// If the restore fails, the database, the backups and the journal are kept and the next start
/// finishes the restore through [recover_interrupted](super::recover_interrupted).
/// The starting revision must match the revision observed by each step; a
/// mismatch fails before that step publishes a backup. With no later
/// definitions, the current revision is checked read-only and no backup is
/// published.
///
/// The terminal scan reads the current directory contents without reusing
/// prior validation results. Scan failure is returned as an unavailable retention diagnostic
/// while preserving the migration result.
pub fn apply_managed_run(
    path: &Path,
    root: &Path,
    definitions: &Definitions<'_>,
    starting_revision: u32,
) -> Result<ManagedRunOutcome, Box<ManagedRunFailure>> {
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
) -> Result<ManagedRunOutcome, Box<ManagedRunFailure>> {
    apply_managed_run_faulty(
        path,
        root,
        definitions,
        starting_revision,
        inspect,
        &mut |_| Ok(()),
    )
}

/// Runs the migration with a hook that can fail or stop the restore at a given stage.
pub(super) fn apply_managed_run_faulty(
    path: &Path,
    root: &Path,
    definitions: &Definitions<'_>,
    starting_revision: u32,
    inspect: impl FnOnce(&Path) -> Result<RetentionDiagnostic, MigrationError>,
    fault: &mut dyn FnMut(RestoreStage) -> Result<(), MigrationError>,
) -> Result<ManagedRunOutcome, Box<ManagedRunFailure>> {
    let mut revision = starting_revision;
    let mut applied_steps = 0;
    let mut failed_step = None;
    let mut failure = match restore::read_journal(root) {
        Ok(None) => None,
        Ok(Some(_)) => Some(MigrationError::Interrupted),
        Err(error) => Some(error),
    };
    // A journal that existed before this run belongs to the earlier unfinished run.
    let pending_journal = failure.is_some();
    let target_version = definitions
        .steps
        .last()
        .map_or(starting_revision, |definition| definition.number);
    let plan = JournalPlan {
        source_version: starting_revision,
        target_version,
    };
    let has_steps = definitions
        .steps
        .iter()
        .any(|definition| definition.number > starting_revision);
    if !has_steps && failure.is_none() {
        failure = migration::verify_noop(path, definitions, starting_revision).err();
    }
    for definition in definitions
        .steps
        .iter()
        .filter(|definition| definition.number > starting_revision)
    {
        if failure.is_some() {
            break;
        }
        let journal = (applied_steps == 0).then_some(&plan);
        match apply_managed_step(path, root, definitions, Some(revision), journal) {
            Ok(step) if step.revision == definition.number => {
                revision = step.revision;
                applied_steps += 1;
            }
            Ok(_) => {
                failed_step = Some(definition.number);
                failure = Some(MigrationError::VersionMismatch);
                break;
            }
            Err(error) => {
                failed_step = Some(definition.number);
                failure = Some(error);
                break;
            }
        }
    }

    let mut restore_status = RestoreStatus::NotNeeded;
    if let Some(error) = &failure {
        if !pending_journal {
            let record = restore::Failure::new(failed_step, error);
            restore_status = restore::restore_after_failure(path, root, record, fault);
        }
    } else if has_steps {
        restore::clear_journal(root);
    }

    let retention = retention_or_unavailable(inspect(root));
    match failure {
        Some(migration) => Err(Box::new(ManagedRunFailure {
            migration,
            restore: restore_status,
            retention,
        })),
        None => Ok(ManagedRunOutcome {
            revision,
            applied_steps,
            retention,
        }),
    }
}
