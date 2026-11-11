use super::*;
use du_core_contracts::visualization::cancel::{
    VizCancelResponse, VizCancelTarget, VizControlResult,
};
use du_job_runtime::CancelResult;
impl Runtime {
    pub(super) fn detach_current(&self, channel: &Id) -> Result<VizControlResult, EngineError> {
        let work = self
            .state
            .lock()
            .map_err(|_| internal())?
            .channels
            .get(channel)
            .ok_or_else(unavailable)?
            .active
            .clone();
        let Some(work) = work else {
            return Ok(VizControlResult::NoEffect);
        };
        let job = work
            .job
            .lock()
            .map_err(|_| internal())?
            .clone()
            .ok_or_else(internal)?;
        let result = self.assembly.jobs.cancel_viz_if(&job, || {
            let mut state = self
                .state
                .lock()
                .map_err(|_| du_job_runtime::RuntimeError::Poisoned)?;
            let mut members = work
                .members
                .lock()
                .map_err(|_| du_job_runtime::RuntimeError::Poisoned)?;
            members.remove(channel);
            if let Some(channel) = state.channels.get_mut(channel) {
                channel.active = None;
            }
            Ok(members.is_empty())
        });
        if matches!(
            result,
            Ok(Some((CancelResult::AlreadyTerminal(_), _)))
                | Err(du_job_runtime::RuntimeError::NotFound)
        ) {
            let mut state = self.state.lock().map_err(|_| internal())?;
            work.members.lock().map_err(|_| internal())?.remove(channel);
            if let Some(channel) = state.channels.get_mut(channel) {
                channel.active = None;
            }
            Ok(VizControlResult::AlreadyTerminal)
        } else {
            result.map_err(runtime_error)?;
            Ok(VizControlResult::Accepted)
        }
    }
    pub(super) fn cancel(&self, target: VizCancelTarget) -> Result<VizCancelResponse, EngineError> {
        let _serial = self.serial.lock().map_err(|_| internal())?;
        let control_result = match target {
            VizCancelTarget::Subscription {
                view_channel,
                request_seq,
            } => {
                let state = self.state.lock().map_err(|_| internal())?;
                let channel = state.channels.get(&view_channel).ok_or_else(unavailable)?;
                if channel.sequence != request_seq.get() {
                    return Ok(VizCancelResponse {
                        control_result: VizControlResult::NoEffect,
                    });
                }
                drop(state);
                self.detach_current(&view_channel)?
            }
            VizCancelTarget::Job { job_ref } => {
                let state = self.state.lock().map_err(|_| internal())?;
                let owned = state
                    .work
                    .values()
                    .chain(
                        state
                            .channels
                            .values()
                            .filter_map(|channel| channel.active.as_ref()),
                    )
                    .any(|work| {
                        work.job
                            .lock()
                            .is_ok_and(|job| job.as_ref() == Some(&job_ref))
                    });
                drop(state);
                if !owned {
                    return Err(EngineError::new("job.not_found", vec![]));
                }
                match self.assembly.jobs.cancel(&job_ref).map_err(runtime_error)? {
                    CancelResult::Accepted(_) => VizControlResult::Accepted,
                    CancelResult::AlreadyTerminal(_) => VizControlResult::AlreadyTerminal,
                    _ => VizControlResult::NoEffect,
                }
            }
        };
        Ok(VizCancelResponse { control_result })
    }
    pub(super) fn close(&self, channel: &Id) -> Result<(), EngineError> {
        let _serial = self.serial.lock().map_err(|_| internal())?;
        self.detach_current(channel)?;
        self.regions
            .close(&self.owner, channel)
            .map_err(|_| unavailable())?;
        self.state
            .lock()
            .map_err(|_| internal())?
            .channels
            .remove(channel);
        self.reclaim(None)
    }
    pub(super) fn retire(&self) -> Result<(), EngineError> {
        let _serial = self.serial.lock().map_err(|_| internal())?;
        let channels: Vec<_> = self
            .state
            .lock()
            .map_err(|_| internal())?
            .channels
            .keys()
            .cloned()
            .collect();
        for channel in channels {
            self.detach_current(&channel)?;
        }
        let _ = self.regions.close_session(&self.owner);
        let retired = {
            let mut state = self.state.lock().map_err(|_| internal())?;
            state.closed = true;
            state.channels.clear();
            std::mem::take(&mut state.work)
        };
        drop(retired);
        Ok(())
    }
}
