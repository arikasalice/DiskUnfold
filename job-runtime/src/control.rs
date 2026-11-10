use crate::{delivery::enqueue, flows, model::*};
use du_core_contracts::{
    Id,
    job::{JobControl, JobOutcome, JobState, ResourceClass},
};
use std::sync::atomic::Ordering;

impl JobRuntime {
    /// Requests cooperative suspension at each registered flow's safe point.
    pub fn pause(&self, id: &Id) -> Result<(CancelResult, u64), RuntimeError> {
        self.change_pause(id, true)
    }

    /// Resumes the same registered flows from their safe points.
    pub fn resume(&self, id: &Id) -> Result<(CancelResult, u64), RuntimeError> {
        self.change_pause(id, false)
    }

    fn change_pause(&self, id: &Id, pause: bool) -> Result<(CancelResult, u64), RuntimeError> {
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        expire(&mut table, self.clock.now_ms());
        let job = table.jobs.get(id).ok_or(RuntimeError::NotFound)?;
        if job.state.is_terminal() {
            return Ok((CancelResult::AlreadyTerminal(job.state), job.sequence));
        }
        let control = if pause {
            JobControl::Pause
        } else {
            JobControl::Resume
        };
        if !job.descriptor.controls.contains(&control) {
            return Ok((CancelResult::Unsupported(job.state), job.sequence));
        }
        let applicable = if pause {
            job.state == JobState::Running
        } else {
            matches!(job.state, JobState::Paused | JobState::PauseRequested)
        };
        if !applicable {
            return Ok((CancelResult::NoEffect(job.state), job.sequence));
        }
        let state = if pause {
            JobState::PauseRequested
        } else {
            JobState::Running
        };
        let sequence = job.sequence + 1;
        // Queue the transition before committing it so a failed resume cannot strand parked flows.
        enqueue(
            &table,
            Delivery::Event(JobEvent::StateChanged {
                job_id: id.clone(),
                state,
                sequence,
            }),
        )?;
        let job = table.jobs.get_mut(id).ok_or(RuntimeError::NotFound)?;
        job.state = state;
        job.sequence = sequence;
        if pause {
            flows::settle_pause(&mut table, id);
        } else {
            job.wake.notify_all();
        }
        drop(table);
        self.flow_limits.wake_waiters();
        Ok((CancelResult::Accepted(state), sequence))
    }
}

pub(crate) fn final_state(job: &Entry, outcome: &JobOutcome) -> JobState {
    if job.cancel.0.load(Ordering::Acquire) {
        return JobState::Cancelled;
    }
    if outcome.completed_scope {
        if job
            .descriptor
            .resource_classes
            .contains(&ResourceClass::FileChange)
            && outcome.counts.completed > 0
            && (outcome.counts.failed > 0 || outcome.counts.skipped > 0)
        {
            return JobState::Partial;
        }
        return JobState::Done;
    }
    if outcome.counts.completed > 0 || outcome.counts.overwritten > 0 {
        JobState::Partial
    } else {
        JobState::Failed
    }
}
