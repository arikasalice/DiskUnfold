//! GUI view channels: the latest request sequence and its running work.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use du_core_contracts::{Id, group_ipc::GroupCancelOutcome};
use du_job_runtime::{CancelResult, JobRuntime};

use super::internal;
use crate::error::EngineError;

/// Provisional channel limit (F36 DLD Q3); the least recently used channel is dropped.
const CHANNEL_LIMIT: usize = 8;

enum Handle {
    Sync(Arc<AtomicBool>),
    Job(Id),
}

struct Channel {
    view: Id,
    last: u32,
    handle: Option<(u32, Handle)>,
}

#[derive(Default)]
pub(super) struct Channels(Mutex<Vec<Channel>>);

impl Channels {
    /// Admits a request; a larger sequence cancels the channel's earlier work.
    /// Returns the cancellation flag of this request.
    pub(super) fn admit(
        &self,
        view: &Id,
        seq: u32,
        runtime: &JobRuntime,
    ) -> Result<Arc<AtomicBool>, EngineError> {
        let mut channels = self.0.lock().map_err(|_| internal())?;
        let flag = Arc::new(AtomicBool::new(false));
        let position = channels.iter().position(|channel| &channel.view == view);
        let mut channel = match position {
            Some(index) => channels.remove(index),
            None => Channel {
                view: view.clone(),
                last: 0,
                handle: None,
            },
        };
        if seq > channel.last || position.is_none() {
            if let Some((_, handle)) = channel.handle.take() {
                cancel(&handle, runtime);
            }
            channel.last = seq;
        }
        if seq == channel.last {
            channel.handle = Some((seq, Handle::Sync(flag.clone())));
        }
        channels.push(channel);
        if channels.len() > CHANNEL_LIMIT {
            channels.remove(0);
        }
        Ok(flag)
    }

    /// Records the job that now carries the channel's latest request.
    pub(super) fn set_job(&self, view: &Id, seq: u32, job: Id) -> Result<(), EngineError> {
        let mut channels = self.0.lock().map_err(|_| internal())?;
        if let Some(channel) = channels.iter_mut().find(|channel| &channel.view == view)
            && channel.last == seq
        {
            channel.handle = Some((seq, Handle::Job(job)));
        }
        Ok(())
    }

    /// Forgets finished work of a request without cancelling it.
    pub(super) fn finish(&self, view: &Id, seq: u32) {
        if let Ok(mut channels) = self.0.lock()
            && let Some(channel) = channels.iter_mut().find(|channel| &channel.view == view)
            && channel
                .handle
                .as_ref()
                .is_some_and(|(known, _)| *known == seq)
        {
            channel.handle = None;
        }
    }

    /// Cancels the channel's work for this exact sequence.
    pub(super) fn cancel(
        &self,
        view: &Id,
        seq: u32,
        runtime: &JobRuntime,
    ) -> Result<GroupCancelOutcome, EngineError> {
        let mut channels = self.0.lock().map_err(|_| internal())?;
        let Some(channel) = channels.iter_mut().find(|channel| &channel.view == view) else {
            return Ok(GroupCancelOutcome::NoEffect);
        };
        match channel.handle.take() {
            Some((known, handle)) if known == seq => Ok(cancel(&handle, runtime)),
            other => {
                channel.handle = other;
                Ok(GroupCancelOutcome::NoEffect)
            }
        }
    }
}

fn cancel(handle: &Handle, runtime: &JobRuntime) -> GroupCancelOutcome {
    match handle {
        Handle::Sync(flag) => {
            flag.store(true, Ordering::Release);
            GroupCancelOutcome::Accepted
        }
        Handle::Job(job) => outcome(runtime.cancel(job).ok()),
    }
}

/// Maps an F24 control result to the published group control outcome.
pub(super) fn outcome(result: Option<CancelResult>) -> GroupCancelOutcome {
    match result {
        Some(CancelResult::Accepted(_)) => GroupCancelOutcome::Accepted,
        Some(CancelResult::AlreadyTerminal(_)) => GroupCancelOutcome::AlreadyTerminal,
        Some(CancelResult::NoEffect(_) | CancelResult::Unsupported(_)) | None => {
            GroupCancelOutcome::NoEffect
        }
    }
}
