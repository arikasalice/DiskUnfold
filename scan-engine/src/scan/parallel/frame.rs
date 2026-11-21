//! Directory work items and the verified handles they share.
//!
//! NFR3.HD1.app-host.DD1: a queued child directory carries only its path (whose
//! length is its depth), its observed entry and a shared reference to the parent
//! handle that enumerated it. Its priority class is derived from that path when
//! it is queued. The child is opened relative to the retained parent handle when
//! a worker of the same lane is assigned to it, so queued work never resolves a
//! path again and the admission and identity binding of the entry stay intact.
//! The open is a turn of its own: the opened directory returns to its lane and
//! its first read is assigned again, in the current priority order.
use super::admission::Boundary;
use super::correlated_budget::Permit;
use crate::scan_diagnostics::{DirectoryFrameMetrics, FrameAccounting};
pub(super) mod correlated;
use du_core_contracts::{
    ports::PortError,
    scan_observation::{AnchoredDirectory, ObservedEntry, TimedObservedEntry},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

/// How often a wait for a busy handle checks its stop condition.
const STOP_CHECK: Duration = Duration::from_millis(25);

/// A verified directory handle shared by its frame and its queued children.
///
/// One adapter call at a time holds the handle, only for the duration of that
/// call. No scheduler lock is taken while it is held, and it is never held
/// across a channel send, so calls on one handle never wait on another lane.
/// The port reads a batch through `&mut self` and opens a child through
/// `&self`, so a child open still waits for a read of its parent in progress;
/// that wait can stop (see [`Directory::open`]).
#[derive(Clone)]
pub(super) struct Anchor(Arc<Handle>);

struct Handle {
    slot: Mutex<Slot>,
    returned: Condvar,
}

enum Slot {
    Free(Box<dyn AnchoredDirectory>),
    /// An adapter call holds the handle.
    InUse,
    /// An adapter call on the handle panicked. The handle is kept unused until
    /// the last reference to it is dropped, so unwinding does not close it.
    Broken {
        _retained: Option<Box<dyn AnchoredDirectory>>,
    },
}

/// Why an adapter call on a shared handle did not run.
enum Unrun {
    /// The stop condition held while another call held the handle.
    Stopped,
    /// An earlier call on the handle panicked.
    Unavailable,
}

/// The handle, taken out of its slot for one adapter call.
struct Lease<'a> {
    handle: &'a Handle,
    directory: Option<Box<dyn AnchoredDirectory>>,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let mut slot = lock(&self.handle.slot);
        *slot = match self.directory.take() {
            Some(directory) if !std::thread::panicking() => Slot::Free(directory),
            directory => Slot::Broken {
                _retained: directory,
            },
        };
        drop(slot);
        self.handle.returned.notify_all();
    }
}

fn lock(slot: &Mutex<Slot>) -> MutexGuard<'_, Slot> {
    // The slot lock is never held across an adapter call, so it is not
    // poisoned by an adapter panic; a poisoned slot still holds a valid state.
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Anchor {
    fn new(directory: Box<dyn AnchoredDirectory>) -> Self {
        Self(Arc::new(Handle {
            slot: Mutex::new(Slot::Free(directory)),
            returned: Condvar::new(),
        }))
    }

    /// Runs one adapter call on the handle once no other call holds it.
    ///
    /// While another call holds the handle, `stop` is checked each time the
    /// wait wakes and finds the handle still in use, at least every
    /// [`STOP_CHECK`], without holding any lock.
    /// Returns [`Unrun::Unavailable`] when an earlier call on this handle
    /// panicked; the scan is then already being aborted by the worker that
    /// observed the panic.
    fn with<R>(
        &self,
        stop: &dyn Fn() -> bool,
        call: impl FnOnce(&mut dyn AnchoredDirectory) -> R,
    ) -> Result<R, Unrun> {
        let mut lease = self.lease(stop)?;
        let directory = lease.directory.as_deref_mut().ok_or(Unrun::Unavailable)?;
        Ok(super::lane_meter::os_call(|| call(directory)))
    }

    fn lease(&self, stop: &dyn Fn() -> bool) -> Result<Lease<'_>, Unrun> {
        let handle = &*self.0;
        let mut slot = lock(&handle.slot);
        loop {
            match std::mem::replace(&mut *slot, Slot::InUse) {
                Slot::Free(directory) => {
                    return Ok(Lease {
                        handle,
                        directory: Some(directory),
                    });
                }
                Slot::InUse => {}
                broken @ Slot::Broken { .. } => {
                    *slot = broken;
                    return Err(Unrun::Unavailable);
                }
            }
            slot = match handle.returned.wait_timeout(slot, STOP_CHECK) {
                Ok((slot, _)) => slot,
                Err(error) => error.into_inner().0,
            };
            if matches!(*slot, Slot::InUse) {
                drop(slot);
                if stop() {
                    return Err(Unrun::Stopped);
                }
                slot = lock(&handle.slot);
            }
        }
    }
}

/// The handle state of one directory work item.
pub(super) enum Directory {
    /// An opened directory whose handle may be shared with queued children.
    Opened(Anchor),
    /// An admitted child that is opened from `parent` when it is assigned.
    Pending {
        parent: Anchor,
        entry: Box<ObservedEntry>,
        _entry_permit: Option<Permit>,
    },
}

/// Why a pending directory could not be opened.
pub(super) enum OpenFailure {
    /// The adapter rejected the open; it is recorded as a collection failure.
    Port(PortError),
    /// The parent handle is unusable after a panic in another adapter call.
    Unavailable,
    /// The stop condition held while a call on the parent handle was in
    /// progress. The child was not opened and is still pending.
    Stopped,
}

/// An observation stays paired with its attempt-scoped child ID while queued.
pub(super) struct QueuedEntry {
    pub(super) timed: TimedObservedEntry,
    pub(super) child_id: Option<u64>,
    pub(super) child_attempt: Option<correlated::Attempt>,
    pub(super) payload_permit: Option<Permit>,
}

impl From<TimedObservedEntry> for QueuedEntry {
    fn from(timed: TimedObservedEntry) -> Self {
        Self {
            timed,
            child_id: None,
            child_attempt: None,
            payload_permit: None,
        }
    }
}

impl Directory {
    /// Wraps a handle that the caller already opened and verified.
    pub(super) fn opened(directory: Box<dyn AnchoredDirectory>) -> Self {
        Self::Opened(Anchor::new(directory))
    }

    /// A queued child of `parent`; no OS call is made until it is opened.
    pub(super) fn pending(
        parent: Anchor,
        entry: ObservedEntry,
        entry_permit: Option<Permit>,
    ) -> Self {
        Self::Pending {
            parent,
            entry: Box::new(entry),
            _entry_permit: entry_permit,
        }
    }

    pub(super) fn is_pending(&self) -> bool {
        matches!(self, Self::Pending { .. })
    }

    /// The opened handle, shared with children queued from it.
    pub(super) fn anchor(&self) -> Option<&Anchor> {
        match self {
            Self::Opened(anchor) => Some(anchor),
            Self::Pending { .. } => None,
        }
    }

    /// Runs one adapter call on an opened handle; `None` if it is not usable.
    ///
    /// Waits without a stop condition while another call holds the handle.
    pub(super) fn with<R>(&self, call: impl FnOnce(&mut dyn AnchoredDirectory) -> R) -> Option<R> {
        self.anchor()
            .and_then(|anchor| anchor.with(&|| false, call).ok())
    }

    /// Opens a pending child relative to its retained parent handle.
    ///
    /// The adapter revalidates the observed entry. On success the parent
    /// reference is released; an opened directory is left unchanged.
    ///
    /// When another call holds the parent handle, typically the parent's own
    /// read, the open waits for it and returns [`OpenFailure::Stopped`] as soon
    /// as `stop` holds, so a stalled parent read does not hold a cancelled
    /// worker. `stop` is called without holding any lock.
    pub(super) fn open(&mut self, stop: &dyn Fn() -> bool) -> Result<(), OpenFailure> {
        let Self::Pending { parent, entry, .. } = self else {
            return Ok(());
        };
        let opened = parent
            .with(stop, |directory| directory.open_child(entry))
            .map_err(|unrun| match unrun {
                Unrun::Stopped => OpenFailure::Stopped,
                Unrun::Unavailable => OpenFailure::Unavailable,
            })?
            .map_err(OpenFailure::Port)?;
        *self = Self::opened(opened);
        Ok(())
    }
}

/// One unit of directory work in a lane: a pending child or an opened directory.
pub(super) struct Frame {
    // Prepared per-frame state; activation waits for the receiver contract.
    pub(super) correlated: correlated::Attempt,
    pub(super) root_index: u32,
    pub(super) path: Vec<Vec<u8>>,
    _path_permit: Option<Permit>,
    pub(super) directory: Directory,
    pub(super) entries: VecDeque<QueuedEntry>,
    pub(super) deferred: VecDeque<QueuedEntry>,
    pub(super) root: bool,
    pub(super) first_batch: bool,
    pub(super) root_published: bool,
    pub(super) batch_pending: bool,
    pub(super) case_observed: bool,
    pub(super) boundary: Boundary,
    /// Entries an exclusion rule dropped directly below this directory so
    /// far, over all of its batches. It is reported once, when the listing
    /// ends or stops; a name the rule could not decide is not counted.
    pub(super) excluded: u64,
    accounting: Option<Box<FrameAccounting>>,
}

impl Frame {
    /// A scope root whose handle was opened and verified at scan admission.
    ///
    /// `boundary` comes from the root identity reserved, in root order, before
    /// any worker started.
    pub(super) fn root(
        root_index: u32,
        directory: Box<dyn AnchoredDirectory>,
        boundary: Boundary,
    ) -> Self {
        let mut frame =
            Self::with_directory(root_index, Vec::new(), Directory::opened(directory), true);
        frame.boundary = boundary;
        frame
    }

    /// An admitted child directory waiting to be opened from `parent`.
    pub(super) fn child(
        root_index: u32,
        path: Vec<Vec<u8>>,
        parent: Anchor,
        entry: ObservedEntry,
        entry_permit: Option<Permit>,
        path_permit: Option<Permit>,
        context: (correlated::Attempt, &super::admission::Visited, Boundary),
    ) -> Self {
        let (correlated, visited, parent_boundary) = context;
        let boundary = visited.child_boundary(&entry, parent_boundary);
        let mut frame = Self::with_directory(
            root_index,
            path,
            Directory::pending(parent, entry, entry_permit),
            false,
        );
        frame._path_permit = path_permit;
        frame.correlated = correlated;
        frame.boundary = boundary;
        frame
    }

    fn with_directory(
        root_index: u32,
        path: Vec<Vec<u8>>,
        directory: Directory,
        root: bool,
    ) -> Self {
        Self {
            correlated: correlated::Attempt::default(),
            root_index,
            path,
            _path_permit: None,
            directory,
            entries: VecDeque::new(),
            deferred: VecDeque::new(),
            root,
            first_batch: false,
            root_published: false,
            batch_pending: false,
            case_observed: false,
            boundary: Boundary::Unknown,
            excluded: 0,
            accounting: None,
        }
    }

    /// Distance from the scope root; the root itself has depth zero.
    pub(super) fn depth(&self) -> usize {
        self.path.len()
    }

    /// Starts accounting this frame until it is dropped, including while a worker owns it.
    pub(super) fn track(&mut self, metrics: Option<&Arc<DirectoryFrameMetrics>>) {
        if let Some(metrics) = metrics {
            self.accounting = Some(Box::new(FrameAccounting::new(
                Arc::clone(metrics),
                path_heap_bytes(&self.path),
            )));
        }
    }
}

fn path_heap_bytes(path: &Vec<Vec<u8>>) -> usize {
    path.capacity()
        .saturating_mul(std::mem::size_of::<Vec<u8>>())
        .saturating_add(path.iter().fold(0_usize, |bytes, component| {
            bytes.saturating_add(component.capacity())
        }))
}
