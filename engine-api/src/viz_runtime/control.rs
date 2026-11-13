use super::*;
use du_core_contracts::visualization::cancel::{
    VizCancelResponse, VizCancelTarget, VizControlResult,
};
use du_job_runtime::CancelResult;
enum SubscriptionChange<'a> {
    Cancel,
    Close,
    Replace { sequence: u32, work: &'a Arc<Work> },
}
impl SubscriptionChange<'_> {
    fn apply(&self, channel: &mut ChannelState) {
        match self {
            Self::Cancel | Self::Close => channel.active = None,
            Self::Replace { sequence, work } => {
                channel.sequence = *sequence;
                channel.active = Some((*work).clone());
            }
        }
    }
}
impl Runtime {
    pub(super) fn replace_subscription(
        &self,
        request: &VizRenderRequest,
        work: &Arc<Work>,
    ) -> Result<(), EngineError> {
        self.change_subscription(
            &request.view_channel,
            SubscriptionChange::Replace {
                sequence: request.request_seq.get(),
                work,
            },
        )
        .map(|_| ())
    }

    // Call with the serial lock held. Replacement has already staged the new
    // membership and released its gate. Only the predecessor's gate is held
    // while changing the channel pointer and removing predecessor membership.
    fn change_subscription(
        &self,
        channel: &Id,
        change: SubscriptionChange<'_>,
    ) -> Result<VizControlResult, EngineError> {
        let previous = {
            let mut state = self.state.lock().map_err(|_| internal())?;
            let current = state.channels.get_mut(channel).ok_or_else(unavailable)?;
            let previous = current.active.clone();
            let same_work = matches!(&change, SubscriptionChange::Replace { work, .. }
                if previous.as_ref().is_some_and(|old| Arc::ptr_eq(old, work)));
            if previous.is_none() || same_work {
                change.apply(current);
                return Ok(VizControlResult::NoEffect);
            }
            previous
        };
        let previous = previous.ok_or_else(internal)?;
        let job = previous.job_id()?;
        let update = || {
            use du_job_runtime::RuntimeError;
            let mut state = self.state.lock().map_err(|_| RuntimeError::Poisoned)?;
            let current = state
                .channels
                .get_mut(channel)
                .ok_or(RuntimeError::InvalidState)?;
            let mut members = previous
                .members
                .lock()
                .map_err(|_| RuntimeError::Poisoned)?;
            members.remove(channel);
            change.apply(current);
            Ok(members.is_empty())
        };
        let result = self.assembly.jobs.cancel_viz_if(&job, update);
        match result {
            Ok(Some((CancelResult::AlreadyTerminal(_), _)))
            | Err(du_job_runtime::RuntimeError::NotFound) => {
                if !matches!(change, SubscriptionChange::Cancel) {
                    update().map_err(runtime_error)?;
                }
                Ok(VizControlResult::AlreadyTerminal)
            }
            // F24 sets cancellation and records the state before reporting
            // delivery failure. The subscription change has already won.
            Ok(_) | Err(du_job_runtime::RuntimeError::DeliveryUnavailable) => {
                Ok(VizControlResult::Accepted)
            }
            Err(error) => Err(runtime_error(error)),
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
                self.change_subscription(&view_channel, SubscriptionChange::Cancel)?
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
        self.change_subscription(channel, SubscriptionChange::Close)?;
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
            self.change_subscription(&channel, SubscriptionChange::Close)?;
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
