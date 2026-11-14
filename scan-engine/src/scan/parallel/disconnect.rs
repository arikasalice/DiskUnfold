//! Collection shutdown keys are distinct from device concurrency groups.
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};
// The sequential collector closes its directories on the same evidence.
pub(super) use super::super::issues::{
    DISCONNECTED as REASON, disconnected as is_error, disconnected_entry as is_entry,
};

/// Closes reads after a disconnection, one volume identity at a time
/// (FR12.HD1.app-host.DD1).
///
/// The unit that closes is the volume identity retained at scan admission, and
/// roots on the same volume close together. A root on another volume of the
/// same physical device, and a root without a volume identity, keep being
/// read; each closes when one of its own calls reports a disconnection.
/// Device groups, which share lanes and I/O permits, do not decide what closes.
/// Closing by physical device would end such a scan sooner, but it could close
/// a partition that is still connected and lose the result read from it. The
/// cost of the volume unit is that a scan of a removed device can end somewhat
/// later, after the roots on its other volumes meet their own failing calls.
pub(super) struct Disconnects {
    lanes: Vec<usize>,
    stopped: Vec<AtomicBool>,
}

impl Disconnects {
    /// Builds the shutdown keys from volume identities retained at scan admission.
    ///
    /// Roots with the same non-empty volume identity share one closed mark; a
    /// root without a volume identity has a mark of its own.
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

    /// Closes the volume identity of `root`, and with it every root on that
    /// volume. Roots on other volumes, of the same physical device included,
    /// stay open.
    pub(super) fn close(&self, root: u32) {
        self.stopped[self.lanes[root as usize]].store(true, Ordering::Release);
    }

    /// Closes the volume of `root` when `reason` is a disconnection.
    pub(super) fn observe_reason(&self, root: u32, reason: &str) {
        if reason == REASON {
            self.close(root);
        }
    }

    /// Whether the volume identity of `root` was closed.
    ///
    /// An operation that passes this admission check may finish even if the volume closes later.
    pub(super) fn closed(&self, root: u32) -> bool {
        self.stopped[self.lanes[root as usize]].load(Ordering::Acquire)
    }

    /// Whether any volume identity of this scan was closed.
    pub(super) fn any_closed(&self) -> bool {
        self.stopped
            .iter()
            .any(|state| state.load(Ordering::Acquire))
    }
}
