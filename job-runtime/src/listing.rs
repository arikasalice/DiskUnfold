use crate::model::{JobPriority, JobRuntime, JobSnapshot, RuntimeError, expire};
use du_core_contracts::{
    Id,
    job::{JobControl, JobState},
};

/// A wait reason supported by evidence currently retained in the job table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobWaitReason {
    Paused,
    QueueFull,
    None,
    Unknown,
}

/// Result of atomically confirming that no unconfirmed `change.*` job blocks quit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionClose {
    /// No new work can be submitted; the caller may proceed with shutdown.
    Accepted {
        /// Nonterminal `change.*` jobs present at the admission cut.
        active_change_jobs: usize,
    },
    /// At least one nonterminal `change.*` job needs explicit user confirmation.
    ConfirmRequired {
        /// Nonterminal `change.*` jobs present at the unchanged admission cut.
        active_change_jobs: usize,
    },
    /// Admission had already been closed by an earlier shutdown request.
    AlreadyClosed,
}

/// Nonterminal job totals read at one cut of the job table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActiveJobCounts {
    /// Every job that has not reached a terminal state.
    pub total: usize,
    /// The nonterminal jobs whose kind starts with `change.`.
    pub change: usize,
}

pub(crate) fn wait_reason(state: JobState) -> JobWaitReason {
    match state {
        JobState::Paused => JobWaitReason::Paused,
        JobState::Queued => JobWaitReason::QueueFull,
        JobState::Done | JobState::Partial | JobState::Failed | JobState::Cancelled => {
            JobWaitReason::None
        }
        JobState::Running | JobState::PauseRequested | JobState::CancelRequested => {
            JobWaitReason::Unknown
        }
    }
}

/// One job read from the runtime table at the same cut as the other entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobListEntry {
    pub job_id: Id,
    pub snapshot: JobSnapshot,
    pub scope_summary: String,
    pub priority: JobPriority,
    pub wait_reason: JobWaitReason,
    /// Controls declared by the job kind, independent of the current state.
    pub supported_controls: Vec<JobControl>,
}

impl JobRuntime {
    /// Counts nonterminal jobs and, of those, the nonterminal `change.*` jobs.
    ///
    /// Works before and after admission closes, so the host can tell the user how
    /// much work is left without taking a shutdown decision.
    pub fn active_job_counts(&self) -> Result<ActiveJobCounts, RuntimeError> {
        let now = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        expire(&mut table, now);
        let mut counts = ActiveJobCounts::default();
        for job in table.jobs.values().filter(|job| !job.state.is_terminal()) {
            counts.total += 1;
            if job.descriptor.kind_id.starts_with("change.") {
                counts.change += 1;
            }
        }
        Ok(counts)
    }

    /// Atomically checks active `change.*` job kinds and, if allowed, closes admission.
    ///
    /// `confirmed` is supplied only after the GUI has confirmed leaving active
    /// file-change work. A `ConfirmRequired` result changes no runtime state;
    /// an accepted result prevents concurrent or later submissions before the
    /// caller proceeds to gateway closure.
    pub fn close_admission_for_shutdown(
        &self,
        confirmed: bool,
    ) -> Result<AdmissionClose, RuntimeError> {
        let now = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        if table.shutting_down {
            return Ok(AdmissionClose::AlreadyClosed);
        }
        expire(&mut table, now);
        let active_change_jobs = table
            .jobs
            .values()
            .filter(|job| !job.state.is_terminal() && job.descriptor.kind_id.starts_with("change."))
            .count();
        if active_change_jobs > 0 && !confirmed {
            return Ok(AdmissionClose::ConfirmRequired { active_change_jobs });
        }
        table.shutting_down = true;
        Ok(AdmissionClose::Accepted { active_change_jobs })
    }

    /// Reads one consistent table cut in submission order. An empty filter matches no jobs.
    pub fn list(&self, states: Option<&[JobState]>) -> Result<Vec<JobListEntry>, RuntimeError> {
        let now = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        expire(&mut table, now);
        let mut rows: Vec<_> = table
            .jobs
            .iter()
            .filter(|(_, job)| states.is_none_or(|filter| filter.contains(&job.state)))
            .map(|(id, job)| {
                (
                    job.order,
                    JobListEntry {
                        job_id: id.clone(),
                        snapshot: job.snapshot(),
                        scope_summary: job.submission.scope_summary.clone(),
                        priority: job.submission.priority,
                        wait_reason: wait_reason(job.state),
                        supported_controls: job.descriptor.controls.clone(),
                    },
                )
            })
            .collect();
        drop(table);
        rows.sort_unstable_by_key(|(order, _)| *order);
        Ok(rows.into_iter().map(|(_, row)| row).collect())
    }

    /// Counts nonterminal jobs that declare the given resource class at one table cut.
    pub fn active_count_for_resource_class(
        &self,
        resource_class: du_core_contracts::job::ResourceClass,
    ) -> Result<usize, RuntimeError> {
        let now = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        expire(&mut table, now);
        Ok(table
            .jobs
            .values()
            .filter(|job| {
                !job.state.is_terminal()
                    && job.descriptor.resource_classes.contains(&resource_class)
            })
            .count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_do_not_infer_unobserved_waits() {
        for state in [
            JobState::Running,
            JobState::PauseRequested,
            JobState::CancelRequested,
        ] {
            assert_eq!(wait_reason(state), JobWaitReason::Unknown);
        }
        assert_eq!(wait_reason(JobState::Queued), JobWaitReason::QueueFull);
        assert_eq!(wait_reason(JobState::Paused), JobWaitReason::Paused);
        for state in [
            JobState::Done,
            JobState::Partial,
            JobState::Failed,
            JobState::Cancelled,
        ] {
            assert_eq!(wait_reason(state), JobWaitReason::None);
        }
    }
}
