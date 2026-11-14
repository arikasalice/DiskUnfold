//! GUI view channels: the latest request sequence and its running work.
//!
//! Lock order: the channel lock is taken before any job runtime lock. The runtime never
//! calls back into a channel, and the job publication callback does not touch channels, so
//! the runtime may be called while the channel lock is held.
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
    /// Returns the cancellation flag of this request. When the channel limit is exceeded,
    /// the least recently used channel is dropped and its work released.
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
                release(&handle, &channels, runtime);
            }
            channel.last = seq;
        }
        if seq == channel.last {
            channel.handle = Some((seq, Handle::Sync(flag.clone())));
        }
        channels.push(channel);
        if channels.len() > CHANNEL_LIMIT {
            let dropped = channels.remove(0);
            if let Some((_, handle)) = dropped.handle {
                release(&handle, &channels, runtime);
            }
        }
        Ok(flag)
    }

    /// Submits the job of the channel's request and binds it to the channel in one step.
    ///
    /// `submit` runs under the channel lock and returns the job that carries the request
    /// (new or deduplicated) with its classification reference, registering whatever
    /// lets another view find that reference. Because the lock is held from the submission
    /// until the channel holds the job, no other view can receive the job and cancel it
    /// before this view is counted as a holder. When the view has meanwhile moved on to a
    /// later sequence, the job is released at once, and cancelled unless another view holds it.
    pub(super) fn bind_submission(
        &self,
        (view, seq): (&Id, u32),
        runtime: &JobRuntime,
        submit: impl FnOnce() -> Result<(Id, Id), EngineError>,
    ) -> Result<(Id, Id), EngineError> {
        let mut channels = self.0.lock().map_err(|_| internal())?;
        let (job, reference) = submit()?;
        match channels
            .iter_mut()
            .find(|channel| &channel.view == view && channel.last == seq)
        {
            Some(channel) => channel.handle = Some((seq, Handle::Job(job.clone()))),
            None => {
                release(&Handle::Job(job.clone()), &channels, runtime);
            }
        }
        Ok((job, reference))
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

    /// Cancels the channel's work for this exact sequence. A job that another view
    /// still waits for keeps running; only this view's interest is released.
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
        let handle = match channel.handle.take() {
            Some((known, handle)) if known == seq => handle,
            other => {
                channel.handle = other;
                return Ok(GroupCancelOutcome::NoEffect);
            }
        };
        Ok(release(&handle, &channels, runtime))
    }
}

/// Cancels released work unless a channel in `others` still waits for the same running job,
/// so that views sharing one deduplicated job do not cancel each other. A job that has
/// ended, or that the runtime no longer knows, has no waiting view.
fn release(handle: &Handle, others: &[Channel], runtime: &JobRuntime) -> GroupCancelOutcome {
    if let Handle::Job(job) = handle
        && others
            .iter()
            .any(|other| matches!(&other.handle, Some((_, Handle::Job(known))) if known == job))
        && runtime.state(job).is_ok_and(|state| !state.is_terminal())
    {
        return GroupCancelOutcome::Accepted;
    }
    cancel(handle, runtime)
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
