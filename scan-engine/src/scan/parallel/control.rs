//! Job-bound controls over the live directory scheduler.
use super::{Queue, queue};
use std::sync::{Arc, Condvar, Mutex, Weak};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanPriorityOutcome {
    /// The target was installed for subsequent directory assignment.
    Accepted { remaining_directories: usize },
    /// The request repeated the current area or the area has no outstanding work.
    NoEffect {
        same_region: bool,
        remaining_directories: usize,
    },
}

struct Attachment {
    queue: Option<Weak<Mutex<Queue>>>,
    wake: Option<Weak<Condvar>>,
    terminal: bool,
}

/// State shared by a scan body and its job-bound control handle.
pub(crate) struct ScanControlSession {
    root_count: usize,
    attachment: Mutex<Attachment>,
}

impl ScanControlSession {
    pub(crate) fn new(root_count: usize) -> Self {
        Self {
            root_count,
            attachment: Mutex::new(Attachment {
                queue: None,
                wake: None,
                terminal: false,
            }),
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
}
