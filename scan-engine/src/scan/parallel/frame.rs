//! Directory work items and the verified handles they share.
//!
//! NFR3.HD1.app-host.DD1: a queued child directory carries only its path (whose
//! length is its depth), its observed entry and a shared reference to the parent
//! handle that enumerated it. Its priority class is derived from that path when
//! it is queued. The child is opened relative to the retained parent handle when
//! a worker of the same lane is assigned to it, so queued work never resolves a
//! path again and the admission and identity binding of the entry stay intact.
use super::admission::Boundary;
use du_core_contracts::{
    ports::PortError,
    scan_observation::{AnchoredDirectory, ObservedEntry, TimedObservedEntry},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

/// A verified directory handle shared by its frame and its queued children.
///
/// An adapter call holds this handle's own lock only for the duration of that
/// call. No scheduler lock is taken while it is held, and it is never held
/// across a channel send, so calls on one handle never wait on another lane.
#[derive(Clone)]
pub(super) struct Anchor(Arc<Mutex<Box<dyn AnchoredDirectory>>>);

impl Anchor {
    fn new(directory: Box<dyn AnchoredDirectory>) -> Self {
        Self(Arc::new(Mutex::new(directory)))
    }

    /// Runs one adapter call on the handle.
    ///
    /// Returns `None` when an earlier call on this handle panicked; the scan is
    /// then already being aborted by the worker that observed the panic.
    fn with<R>(&self, call: impl FnOnce(&mut dyn AnchoredDirectory) -> R) -> Option<R> {
        let mut directory = self.0.lock().ok()?;
        Some(call(&mut **directory))
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
    },
}

/// Why a pending directory could not be opened.
pub(super) enum OpenFailure {
    /// The adapter rejected the open; it is recorded as a collection failure.
    Port(PortError),
    /// The parent handle is unusable after a panic in another adapter call.
    Unavailable,
}

impl Directory {
    /// Wraps a handle that the caller already opened and verified.
    pub(super) fn opened(directory: Box<dyn AnchoredDirectory>) -> Self {
        Self::Opened(Anchor::new(directory))
    }

    /// A queued child of `parent`; no OS call is made until it is opened.
    pub(super) fn pending(parent: Anchor, entry: ObservedEntry) -> Self {
        Self::Pending {
            parent,
            entry: Box::new(entry),
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
    pub(super) fn with<R>(&self, call: impl FnOnce(&mut dyn AnchoredDirectory) -> R) -> Option<R> {
        self.anchor().and_then(|anchor| anchor.with(call))
    }

    /// Opens a pending child relative to its retained parent handle.
    ///
    /// The adapter revalidates the observed entry. On success the parent
    /// reference is released; an opened directory is left unchanged.
    pub(super) fn open(&mut self) -> Result<(), OpenFailure> {
        let Self::Pending { parent, entry } = self else {
            return Ok(());
        };
        let opened = parent
            .with(|directory| directory.open_child(entry))
            .ok_or(OpenFailure::Unavailable)?
            .map_err(OpenFailure::Port)?;
        *self = Self::opened(opened);
        Ok(())
    }
}

/// One unit of directory work in a lane: a pending child or an opened directory.
pub(super) struct Frame {
    pub(super) root_index: u32,
    pub(super) path: Vec<Vec<u8>>,
    pub(super) directory: Directory,
    pub(super) entries: VecDeque<TimedObservedEntry>,
    pub(super) deferred: VecDeque<TimedObservedEntry>,
    pub(super) root: bool,
    pub(super) first_batch: bool,
    pub(super) root_published: bool,
    pub(super) batch_pending: bool,
    pub(super) case_observed: bool,
    pub(super) boundary: Boundary,
}

impl Frame {
    /// A scope root whose handle was opened and verified at scan admission.
    pub(super) fn root(root_index: u32, directory: Box<dyn AnchoredDirectory>) -> Self {
        Self::with_directory(root_index, Vec::new(), Directory::opened(directory), true)
    }

    /// An admitted child directory waiting to be opened from `parent`.
    pub(super) fn child(
        root_index: u32,
        path: Vec<Vec<u8>>,
        parent: Anchor,
        entry: ObservedEntry,
    ) -> Self {
        let boundary = Boundary::child(&entry);
        let mut frame = Self::with_directory(
            root_index,
            path,
            Directory::pending(parent, entry),
            false,
        );
        frame.boundary = boundary;
        frame
    }

    fn with_directory(root_index: u32, path: Vec<Vec<u8>>, directory: Directory, root: bool) -> Self {
        Self {
            root_index,
            path,
            directory,
            entries: VecDeque::new(),
            deferred: VecDeque::new(),
            root,
            first_batch: false,
            root_published: false,
            batch_pending: false,
            case_observed: false,
            boundary: if root {
                Boundary::Pending
            } else {
                Boundary::Unknown
            },
        }
    }

    /// Distance from the scope root; the root itself has depth zero.
    pub(super) fn depth(&self) -> usize {
        self.path.len()
    }
}
