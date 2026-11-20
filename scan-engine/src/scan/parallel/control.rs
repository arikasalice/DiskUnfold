//! Job-bound controls over the live directory scheduler.
use super::{Queue, queue, resource::Condition};
use du_core_contracts::analysis_ipc::{
    ScanResourceMode, ScanSetResourceLane, ScanSetResourceOutcome,
};
use std::sync::{
    Arc, Condvar, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};

/// Why a scan control request was not recorded by the live scheduler.
///
/// An error leaves the scheduling order unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanControlError {
    /// The scan body has not attached its live scheduler yet.
    NotActive,
    /// The scan body has finished and released its scheduler.
    AlreadyTerminal,
    /// The root index is not part of this scan's admitted scope.
    OutOfScope,
    /// The scheduler state could not be read or updated.
    Unavailable,
}

/// How the live scheduler recorded a priority request.
///
/// `Accepted` acknowledges the request; it does not report that any directory
/// of the area was assigned, opened or read. The area applies from the next
/// directory assignment: a directory call already in progress is not
/// interrupted, a paused scan applies it from its first assignment after
/// resuming, and a scan whose cancellation was requested assigns no further
/// directory, so an accepted area can end without any effect.
///
/// Directory work never leaves the storage lane of its root. While the area's
/// lane queues work of the area and has fewer workers than its limit, a worker
/// of another lane moves there when it finishes its current turn. A worker
/// stays in its lane when it is that lane's last worker, or when the work on
/// its overflow stack does not fit in its lane queue; it moves once that work
/// fits. The area can therefore wait for those workers' turns, and a stalled
/// area lane takes at most its own worker limit, never the last worker of
/// another lane. Once the area's lane queues no area work, lent workers return
/// to their own lanes under the same conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanPriorityOutcome {
    /// The area replaced any previous area for later directory assignments.
    Accepted {
        /// Directories at or below the area that were queued or running when
        /// the request was recorded. Directories found later are not counted.
        remaining_directories: usize,
    },
    /// The scheduling order was left unchanged.
    NoEffect {
        /// True when the request repeated the current area; false when the
        /// area has no queued or running directory work.
        same_region: bool,
        /// Directories at or below the area that are queued or running.
        remaining_directories: usize,
    },
}

struct Attachment {
    queue: Option<Weak<Mutex<Queue>>>,
    wake: Option<Weak<Condvar>>,
    terminal: bool,
    condition: Condition,
}

/// State shared by a scan body and its job-bound control handle.
pub(crate) struct ScanControlSession {
    root_count: usize,
    priority_enabled: AtomicBool,
    attachment: Mutex<Attachment>,
}

impl ScanControlSession {
    pub(crate) fn new(root_count: usize) -> Self {
        Self {
            root_count,
            priority_enabled: AtomicBool::new(true),
            attachment: Mutex::new(Attachment {
                queue: None,
                wake: None,
                terminal: false,
                condition: Condition::default(),
            }),
        }
    }

    pub(crate) fn with_resource_condition(
        root_count: usize,
        mode: ScanResourceMode,
        user_limit: Option<u32>,
    ) -> Self {
        let session = Self::new(root_count);
        session.priority_enabled.store(false, Ordering::Release);
        session.initialize_resource_condition(mode, user_limit);
        session
    }

    pub(crate) fn enable_priority(&self) {
        self.priority_enabled.store(true, Ordering::Release);
    }

    pub(super) fn tracks_priority(&self) -> bool {
        self.priority_enabled.load(Ordering::Acquire)
    }

    pub(crate) fn initialize_resource_condition(
        &self,
        mode: ScanResourceMode,
        user_limit: Option<u32>,
    ) {
        let mut attachment = self
            .attachment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if attachment.queue.is_none() && !attachment.terminal {
            attachment.condition = Condition { mode, user_limit };
        }
    }

    pub(crate) fn handle(self: &Arc<Self>) -> ScanControlHandle {
        ScanControlHandle {
            session: Arc::downgrade(self),
        }
    }

    pub(super) fn attach(&self, queue: &Arc<Mutex<Queue>>, wake: &Arc<Condvar>) {
        let mut attachment = self
            .attachment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !attachment.terminal {
            if let Ok(mut queue) = queue.lock() {
                queue.lanes.apply_resource(attachment.condition);
            }
            attachment.queue = Some(Arc::downgrade(queue));
            attachment.wake = Some(Arc::downgrade(wake));
        }
    }

    pub(crate) fn finish(&self) {
        let mut attachment = self
            .attachment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        attachment.queue = None;
        attachment.wake = None;
        attachment.terminal = true;
    }

    fn prioritize(
        &self,
        root_index: u32,
        path: Vec<Vec<u8>>,
    ) -> Result<ScanPriorityOutcome, ScanControlError> {
        if root_index as usize >= self.root_count || path.iter().any(Vec::is_empty) {
            return Err(ScanControlError::OutOfScope);
        }
        let attachment = self
            .attachment
            .lock()
            .map_err(|_| ScanControlError::Unavailable)?;
        if attachment.terminal {
            return Err(ScanControlError::AlreadyTerminal);
        }
        if !self.tracks_priority() {
            return Err(ScanControlError::NotActive);
        }
        let queue = attachment
            .queue
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or(ScanControlError::NotActive)?;
        let wake = attachment
            .wake
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or(ScanControlError::NotActive)?;
        let mut queue = queue.lock().map_err(|_| ScanControlError::Unavailable)?;
        let decision = queue.prioritize(queue::PriorityRegion { root_index, path });
        wake.notify_all();
        Ok(match decision {
            queue::PriorityDecision::Accepted { remaining } => ScanPriorityOutcome::Accepted {
                remaining_directories: remaining,
            },
            queue::PriorityDecision::NoEffect {
                same_region,
                remaining,
            } => ScanPriorityOutcome::NoEffect {
                same_region,
                remaining_directories: remaining,
            },
        })
    }

    fn set_resource(
        &self,
        condition: Condition,
    ) -> Result<(ScanSetResourceOutcome, Vec<ScanSetResourceLane>), ScanControlError> {
        let mut attachment = self
            .attachment
            .lock()
            .map_err(|_| ScanControlError::Unavailable)?;
        if attachment.terminal {
            return Ok((ScanSetResourceOutcome::AlreadyTerminal, Vec::new()));
        }
        if attachment.condition == condition {
            return Ok((
                ScanSetResourceOutcome::NoEffect,
                Self::resource_rows(&attachment)?,
            ));
        }
        let rows = if let Some(queue) = attachment.queue.as_ref().and_then(Weak::upgrade) {
            let mut queue = queue.lock().map_err(|_| ScanControlError::Unavailable)?;
            queue.lanes.apply_resource(condition);
            queue.lanes.resource_rows()
        } else {
            Vec::new()
        };
        attachment.condition = condition;
        if let Some(wake) = attachment.wake.as_ref().and_then(Weak::upgrade) {
            wake.notify_all();
        }
        Ok((ScanSetResourceOutcome::Accepted, rows))
    }

    fn resource_rows(
        attachment: &Attachment,
    ) -> Result<Vec<ScanSetResourceLane>, ScanControlError> {
        let Some(queue) = attachment.queue.as_ref().and_then(Weak::upgrade) else {
            return Ok(Vec::new());
        };
        let queue = queue.lock().map_err(|_| ScanControlError::Unavailable)?;
        Ok(queue.lanes.resource_rows())
    }
}

/// A weak handle bound to one metadata-scan job's live scheduler.
///
/// It never retains the job or scheduler after terminal completion. Callers
/// must resolve the target from the job's current, pinned result and pass only
/// its admitted root index and raw relative components. It consumes those
/// components so admission does not copy path evidence under the result lock.
#[derive(Clone)]
pub struct ScanControlHandle {
    session: Weak<ScanControlSession>,
}

impl ScanControlHandle {
    /// Prioritizes an observed directory region for the next available worker.
    ///
    /// The method reorders existing scan work under `path`; it does not add
    /// roots, enumerate a path itself, or interrupt an OS call already in
    /// progress. A successful count includes active and queued directory work
    /// currently known below the target. The caller must validate the result
    /// version, scope version and folder kind before calling this method. The
    /// raw relative components are consumed by the call.
    ///
    /// Recording a new area moves the queued and stacked work of the previous
    /// and the new area root, in that root's lane, between priority classes
    /// once under the scheduler lock; later claims never scan queued work.
    /// See [`ScanPriorityOutcome`] for what an acknowledgement means.
    ///
    /// # Errors
    /// Returns [`ScanControlError::NotActive`] before the job body attaches,
    /// [`ScanControlError::AlreadyTerminal`] after it finishes,
    /// [`ScanControlError::OutOfScope`] for an invalid root or component list,
    /// and [`ScanControlError::Unavailable`] if scheduler synchronization is
    /// poisoned.
    pub fn prioritize(
        &self,
        root_index: u32,
        path: Vec<Vec<u8>>,
    ) -> Result<ScanPriorityOutcome, ScanControlError> {
        let session = self
            .session
            .upgrade()
            .ok_or(ScanControlError::AlreadyTerminal)?;
        session.prioritize(root_index, path)
    }

    /// Replaces the live resource condition and returns the lane limits observed under the
    /// scheduler lock. A queued job records the condition and applies it when its lanes attach.
    pub fn set_resource(
        &self,
        mode: ScanResourceMode,
        user_limit: Option<u32>,
    ) -> Result<(ScanSetResourceOutcome, Vec<ScanSetResourceLane>), ScanControlError> {
        let Some(session) = self.session.upgrade() else {
            return Ok((ScanSetResourceOutcome::AlreadyTerminal, Vec::new()));
        };
        session.set_resource(Condition { mode, user_limit })
    }
}
