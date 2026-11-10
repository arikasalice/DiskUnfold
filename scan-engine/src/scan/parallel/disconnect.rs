//! Collection shutdown keys are distinct from device concurrency groups.
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::ObservedEntry,
};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

/// Stops only the confirmed volume, or the reporting root if identity is unknown.
pub(super) struct Disconnects {
    lanes: Vec<usize>,
    stopped: Vec<AtomicBool>,
}

pub(super) const REASON: &str = "scan.disconnected";

pub(super) fn is_error(error: &PortError) -> bool {
    error.kind == PortErrorKind::Io && error.reason_key == REASON
}

pub(super) fn is_entry(entry: &ObservedEntry) -> bool {
    match &entry.metadata {
        Err(error) => is_error(error),
        Ok(metadata) => metadata.size.as_ref().is_err_and(is_error),
    }
}

impl Disconnects {
    /// Builds the shutdown keys from volume identities retained at scan admission.
    pub(super) fn new(root_count: usize, volumes: &[Vec<u8>]) -> Self {
        let mut known = HashMap::new();
        let lanes = (0..root_count)
            .map(|root| {
                if let Some(volume) = volumes.get(root).filter(|volume| !volume.is_empty()) {
                    *known.entry(volume).or_insert(root)
                } else {
                    root
                }
            })
            .collect();
        Self {
            lanes,
            stopped: (0..root_count).map(|_| AtomicBool::new(false)).collect(),
        }
    }

    pub(super) fn close(&self, root: u32) {
        self.stopped[self.lanes[root as usize]].store(true, Ordering::Release);
    }

    pub(super) fn observe_reason(&self, root: u32, reason: &str) {
        if reason == REASON {
            self.close(root);
        }
    }

    /// An operation that passes this admission check may finish even if the volume closes later.
    pub(super) fn closed(&self, root: u32) -> bool {
        self.stopped[self.lanes[root as usize]].load(Ordering::Acquire)
    }

    pub(super) fn any_closed(&self) -> bool {
        self.stopped
            .iter()
            .any(|state| state.load(Ordering::Acquire))
    }
}
