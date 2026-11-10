//! Collection shutdown keys are distinct from device concurrency groups.
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

/// Stops only the confirmed volume, or the reporting root if identity is unknown.
pub(super) struct Disconnects {
    lanes: Vec<usize>,
    stopped: Vec<AtomicBool>,
}

impl Disconnects {
    /// Builds the shutdown keys from volume identities retained at scan admission.
    pub(super) fn new(volumes: &[Vec<u8>]) -> Self {
        let mut known = HashMap::new();
        let lanes = volumes
            .iter()
            .enumerate()
            .map(|(root, volume)| {
                if volume.is_empty() {
                    root
                } else {
                    *known.entry(volume).or_insert(root)
                }
            })
            .collect();
        Self {
            lanes,
            stopped: (0..volumes.len()).map(|_| AtomicBool::new(false)).collect(),
        }
    }

    pub(super) fn close(&self, root: u32) {
        self.stopped[self.lanes[root as usize]].store(true, Ordering::Release);
    }

    pub(super) fn observe_reason(&self, root: u32, reason: &str) {
        if reason == "scan.disconnected" {
            self.close(root);
        }
    }

    /// An operation that passes this admission check may finish even if the volume closes later.
    pub(super) fn closed(&self, root: u32) -> bool {
        self.stopped[self.lanes[root as usize]].load(Ordering::Acquire)
    }
}
