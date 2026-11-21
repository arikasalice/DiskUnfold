//! The history.prune descriptor, submitter and bounded job body.

use std::{
    collections::HashSet,
    sync::{Arc, Weak},
};

use du_core_contracts::{
    Id,
    job::{
        Checkpoint, HistoryMode, HistoryPruneCandidate, HistoryPruneDeleteError, JobBody,
        JobContext, JobControl, JobCounts, JobDescriptor, JobOutcome, JobProgress, ProgressUnit,
        ResourceClass,
    },
};
use du_job_runtime::{DeduplicationKey, JobPriority, Submission, SubmitResult};

use super::RetentionPorts;

pub type HistoryPruneTrigger = Arc<dyn Fn() -> Option<Id> + Send + Sync>;

// Provisional until the million-item write-lock measurement resolves DLD Q4.
const MAX_BATCH_ROWS: u64 = 50_000;
const MAX_BATCH_JOBS: usize = 500;

pub(super) fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: "history.prune".into(),
        version: 1,
        display_name_key: "job.history_prune".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["batch_end".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::Output],
        history: HistoryMode::None,
    }
}

pub(super) fn submit<R>(ports: &Arc<RetentionPorts<R>>) -> Option<Id>
where
    R: du_core_contracts::job::HistoryExpiryRead
        + du_core_contracts::job::HistoryPruneRead
        + 'static,
{
    let policy = (ports.policy_source)();
    if !policy.enabled || policy.unavailable_reason.is_some() {
        return None;
    }
    let days = policy.specified_days.unwrap_or(policy.suggested_days);
    let body: Arc<dyn JobBody> = Arc::new(Prune {
        ports: Arc::downgrade(ports),
        expected_policy_fingerprint: policy.policy_fingerprint,
        days,
    });
    let submission = Submission {
        kind_id: "history.prune".into(),
        deduplication_key: Some(DeduplicationKey {
            scope: "history".into(),
            input: "history.prune".into(),
            channel: "app-host".into(),
            key: "history.prune".into(),
        }),
        body,
        priority: JobPriority::Low,
        scope_summary: "history".into(),
        scope_roots: Vec::new(),
        correlation_id: Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes()),
        sample: false,
    };
    match ports.runtime.submit(submission) {
        Ok(SubmitResult::Created(id) | SubmitResult::Existing(id)) => {
            (ports.wake_workers)();
            Some(id)
        }
        Err(error) => {
            eprintln!("history.prune.submit_failed: {error}");
            None
        }
    }
}

struct Prune<R> {
    ports: Weak<RetentionPorts<R>>,
    expected_policy_fingerprint: String,
    days: u32,
}

impl<R> JobBody for Prune<R>
where
    R: du_core_contracts::job::HistoryExpiryRead
        + du_core_contracts::job::HistoryPruneRead
        + 'static,
{
    fn kind_id(&self) -> &str {
        "history.prune"
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let Some(ports) = self.ports.upgrade() else {
            return outcome(
                JobCounts::default(),
                false,
                Some("history.storage_unavailable"),
            );
        };
        let reference_ms = ports.runtime.history_prune_reference_time();
        let mut after = None;
        let mut counts = JobCounts::default();

        loop {
            let policy = (ports.policy_source)();
            if !policy.enabled
                || policy.unavailable_reason.is_some()
                || policy.policy_fingerprint != self.expected_policy_fingerprint
            {
                return outcome(counts, true, None);
            }

            let page = match ports.expiry_reader.lock() {
                Ok(mut reader) => ports.runtime.history_prune_candidates(
                    &mut *reader,
                    self.days,
                    reference_ms,
                    after,
                    MAX_BATCH_JOBS,
                ),
                Err(_) => return outcome(counts, false, Some("history.storage_unavailable")),
            };
            let page = match page {
                Ok(page) => page,
                Err(_) => return outcome(counts, false, Some("history.storage_unavailable")),
            };
            if page.is_empty() {
                return outcome(counts, true, None);
            }
            let batch = match select_batch(page) {
                Ok(batch) => batch,
                Err(()) => return outcome(counts, false, Some("history.storage_unavailable")),
            };
            let Some(last) = batch.last() else {
                return outcome(counts, false, Some("history.storage_unavailable"));
            };
            after = Some(last.position);

            match self.delete_batch(&ports, &batch, reference_ms) {
                Ok((deleted, skipped)) => {
                    counts.completed = counts.completed.saturating_add(deleted);
                    counts.skipped = counts.skipped.saturating_add(skipped);
                    context.report_progress(JobProgress {
                        completed: counts.completed.saturating_add(counts.skipped),
                        total: None,
                    });
                }
                Err(BatchError::PolicyChanged) => return outcome(counts, true, None),
                Err(BatchError::Storage(error)) => {
                    return outcome(counts, false, Some(delete_failure_reason(error)));
                }
            }
            if context.checkpoint() == Checkpoint::Cancel {
                return outcome(counts, false, None);
            }
        }
    }
}

impl<R> Prune<R>
where
    R: du_core_contracts::job::HistoryExpiryRead
        + du_core_contracts::job::HistoryPruneRead
        + 'static,
{
    fn delete_batch(
        &self,
        ports: &RetentionPorts<R>,
        candidates: &[HistoryPruneCandidate],
        reference_ms: i64,
    ) -> Result<(u64, u64), BatchError> {
        let candidate_ids = candidates
            .iter()
            .filter(|candidate| !candidate.has_nonterminal_follow_up)
            .map(|candidate| candidate.job_id.clone())
            .collect::<Vec<_>>();
        let present = if candidate_ids.is_empty() {
            Ok(Vec::new())
        } else {
            ports.undo_info.present(&candidate_ids)
        };
        let present = match present {
            Ok(ids) if ids.iter().all(|id| candidate_ids.contains(id)) => ids,
            _ => {
                return Ok((0, u64::try_from(candidates.len()).unwrap_or(u64::MAX)));
            }
        };
        let present_set: HashSet<Id> = present.iter().cloned().collect();
        let reserved = ports.undo_info.reserve_removal(&present);
        let reserved_set: HashSet<Id> = reserved.iter().cloned().collect();
        if reserved.iter().any(|id| !present_set.contains(id)) {
            ports.undo_info.release(&reserved);
            return Ok((0, u64::try_from(candidates.len()).unwrap_or(u64::MAX)));
        }

        let mut eligible = Vec::new();
        for candidate in candidates {
            if candidate.has_nonterminal_follow_up {
                continue;
            }
            if !present_set.contains(&candidate.job_id) {
                eligible.push(candidate.clone());
                continue;
            }
            if !reserved_set.contains(&candidate.job_id) {
                continue;
            }
            if matches!(ports.undo_info.in_use(&candidate.job_id), Ok(false)) {
                eligible.push(candidate.clone());
            }
        }
        if eligible.is_empty() {
            ports.undo_info.release(&reserved);
            return Ok((0, u64::try_from(candidates.len()).unwrap_or(u64::MAX)));
        }

        let receipt = ports.runtime.prune_history_batch(
            &*ports.prune_delete,
            &self.expected_policy_fingerprint,
            self.days,
            reference_ms,
            &eligible,
        );
        let receipt = match receipt {
            Ok(Some(receipt)) => receipt,
            Ok(None) => {
                ports.undo_info.release(&reserved);
                return Err(BatchError::PolicyChanged);
            }
            Err(error) => {
                ports.undo_info.release(&reserved);
                return Err(BatchError::Storage(error));
            }
        };
        let deleted: HashSet<Id> = receipt.deleted_job_ids.iter().cloned().collect();
        if receipt
            .deleted_job_ids
            .iter()
            .any(|id| !eligible.iter().any(|candidate| &candidate.job_id == id))
        {
            ports.undo_info.release(&reserved);
            return Err(BatchError::Storage(HistoryPruneDeleteError::Unavailable));
        }
        let removed_undo = reserved
            .iter()
            .filter(|id| deleted.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        let released_undo = reserved
            .iter()
            .filter(|id| !deleted.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        ports.undo_info.remove(&removed_undo);
        ports.undo_info.release(&released_undo);
        ports.runtime.publish_history_prune(receipt);
        let deleted_count = u64::try_from(deleted.len()).unwrap_or(u64::MAX);
        let skipped_count = u64::try_from(candidates.len() - deleted.len()).unwrap_or(u64::MAX);
        Ok((deleted_count, skipped_count))
    }
}

enum BatchError {
    PolicyChanged,
    Storage(HistoryPruneDeleteError),
}

fn select_batch(page: Vec<HistoryPruneCandidate>) -> Result<Vec<HistoryPruneCandidate>, ()> {
    let mut batch = Vec::with_capacity(page.len().min(MAX_BATCH_JOBS));
    let mut row_count = 0_u64;
    for candidate in page.into_iter().take(MAX_BATCH_JOBS) {
        if candidate.row_count == 0 {
            return Err(());
        }
        let next_rows = row_count.checked_add(candidate.row_count).ok_or(())?;
        if !batch.is_empty() && next_rows > MAX_BATCH_ROWS {
            break;
        }
        row_count = next_rows;
        let exceeds = row_count > MAX_BATCH_ROWS;
        batch.push(candidate);
        if exceeds || row_count == MAX_BATCH_ROWS {
            break;
        }
    }
    Ok(batch)
}

fn outcome(counts: JobCounts, completed_scope: bool, reason: Option<&str>) -> JobOutcome {
    JobOutcome {
        counts,
        completed_scope,
        failure_reason_key: reason.map(str::to_owned),
        unprocessed_summary: None,
        result_version_id: None,
    }
}

fn delete_failure_reason(error: HistoryPruneDeleteError) -> &'static str {
    match error {
        HistoryPruneDeleteError::Busy => "db.busy",
        HistoryPruneDeleteError::DiskFull => "db.disk_full",
        HistoryPruneDeleteError::Unavailable => "history.storage_unavailable",
    }
}

#[cfg(test)]
mod tests;
