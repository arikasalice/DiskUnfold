use crate::{
    JobRuntime, RuntimeError,
    delivery::enqueue,
    flows::{Context, Services},
    history::HistoryRecorder,
    model::{Delivery, JobEvent},
};
use du_core_contracts::{
    Id,
    job::{HistoryMode, ItemHistoryError, ItemHistoryStart, JobOutcome, JobState, ResourceClass},
};
use std::sync::Arc;

impl JobRuntime {
    /// Runs the oldest eligible queued job on the calling worker thread.
    pub fn run_next(&self) -> Result<Option<Id>, RuntimeError> {
        let now = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        let active = table
            .jobs
            .values()
            .filter(|job| {
                matches!(
                    job.state,
                    JobState::Running
                        | JobState::PauseRequested
                        | JobState::Paused
                        | JobState::CancelRequested
                )
            })
            .count();
        let changing = table.jobs.values().any(|job| {
            matches!(
                job.state,
                JobState::Running
                    | JobState::PauseRequested
                    | JobState::Paused
                    | JobState::CancelRequested
            ) && job
                .descriptor
                .resource_classes
                .contains(&ResourceClass::FileChange)
        });
        if active >= 4 {
            return Ok(None);
        }
        let selected = table
            .jobs
            .iter()
            .filter(|(_, job)| {
                job.state == JobState::Queued
                    && !(changing
                        && job
                            .descriptor
                            .resource_classes
                            .contains(&ResourceClass::FileChange))
            })
            .min_by_key(|(_, job)| (job.submission.priority, job.order))
            .map(|(id, _)| id.clone());
        let Some(id) = selected else {
            return Ok(None);
        };
        let job = table.jobs.get(&id).ok_or(RuntimeError::NotFound)?;
        if job.body.is_none() {
            return Err(RuntimeError::InvalidState);
        }
        let sequence = job.sequence + 1;
        enqueue(
            &table,
            Delivery::Event(JobEvent::StateChanged {
                job_id: id.clone(),
                state: JobState::Running,
                sequence,
            }),
        )?;
        let job = table.jobs.get_mut(&id).ok_or(RuntimeError::NotFound)?;
        job.state = JobState::Running;
        job.started_at_ms = Some(now);
        job.sequence = sequence;
        let progress = job.progress.start();
        let history_start = per_item_start(job, &id, now);
        let cancellation = job.cancel.clone();
        let body = job.body.take().ok_or(RuntimeError::InvalidState)?;
        let publication = job.publication.take();
        let gate = job.publication_gate.clone();
        drop(table);

        let history_recorder = match history_start {
            Some(start) => match self
                .history
                .upgrade()
                .ok_or(ItemHistoryError::Unavailable)
                .and_then(|history| HistoryRecorder::start(history, &start))
            {
                Ok(recorder) => {
                    self.table
                        .lock()
                        .map_err(|_| RuntimeError::Poisoned)?
                        .jobs
                        .get_mut(&id)
                        .ok_or(RuntimeError::NotFound)?
                        .history_started = true;
                    Some(recorder)
                }
                Err(_) => {
                    drop(body);
                    self.finish_with_publication(
                        &id,
                        JobOutcome {
                            counts: Default::default(),
                            completed_scope: false,
                            failure_reason_key: Some("history.record_failed".into()),
                            unprocessed_summary: None,
                            result_version_id: None,
                        },
                        gate,
                        publication,
                    )?;
                    return Ok(Some(id));
                }
            },
            None => None,
        };
        let context = Arc::new(Context::new(
            id.clone(),
            self.table.clone(),
            cancellation,
            Services {
                limits: self.flow_limits.clone(),
                clock: self.clock.clone(),
                progress,
                history_recorder,
            },
        ));
        let mut outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body.run(context.clone())))
                .unwrap_or_else(|_| JobOutcome {
                    counts: Default::default(),
                    completed_scope: false,
                    failure_reason_key: Some("support.internal_error".into()),
                    unprocessed_summary: None,
                    result_version_id: None,
                });
        drop(body);
        if !context.seal_and_wait(&mut outcome.counts) {
            outcome.completed_scope = false;
            outcome.failure_reason_key = Some("support.internal_error".into());
        }
        if context.item_history_failed() {
            outcome.completed_scope = false;
            outcome.failure_reason_key = Some("history.record_failed".into());
            outcome.result_version_id = None;
        }
        self.finish_with_publication(&id, outcome, gate, publication)?;
        Ok(Some(id))
    }
}

fn per_item_start(job: &crate::model::Entry, id: &Id, now: i64) -> Option<ItemHistoryStart> {
    (job.descriptor.history == HistoryMode::PerItem).then(|| ItemHistoryStart {
        job_id: id.clone(),
        kind_id: job.descriptor.kind_id.clone(),
        kind_version: job.descriptor.version,
        scope_summary: job.submission.scope_summary.clone(),
        started_at_ms: now,
        plan_version_id: None,
        plan_lineage_id: None,
        plan_hash: None,
        predecessor_job_id: None,
        relation_kind: None,
    })
}
