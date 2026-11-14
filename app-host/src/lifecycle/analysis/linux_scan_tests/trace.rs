//! The real Linux scope roots, wrapped to record filter decisions and opens.
use super::*;
use du_core_contracts::{
    Timestamp,
    ports::PortError,
    scan_observation::{
        AnchoredDirectory, DirectoryCaseEvidence, DirectoryObservations, FilterDecision,
        FilteredBatch, HandleIdentity, ObservedEntry, OpenedScopeRoot, TimedFilteredBatch,
        TimedObservedEntry, VolumeSpaceReader,
    },
};
use du_platform_os::linux_observation::LinuxScopeRoots;
use std::sync::Mutex;

type Clock<'a> = &'a dyn Fn() -> Option<Timestamp>;

/// Names offered to the scan filter and directories opened, in any traced directory.
#[derive(Default)]
pub(super) struct Trace {
    pub(super) offered: Mutex<Vec<(Vec<u8>, FilterDecision)>>,
    pub(super) opened: Mutex<Vec<Vec<u8>>>,
}

impl Trace {
    /// Every filter decision made for `name`, in order.
    pub(super) fn decisions(&self, name: &[u8]) -> Result<Vec<FilterDecision>, String> {
        Ok(self
            .offered
            .lock()
            .map_err(|_| "trace poisoned")?
            .iter()
            .filter(|(seen, _)| seen == name)
            .map(|(_, decision)| *decision)
            .collect())
    }
}

/// The real Linux scope roots, with every directory wrapped by [`Traced`].
pub(super) struct TracedRoots(pub(super) Arc<Trace>);

impl ScopeRootPort for TracedRoots {
    fn open_root(&self, path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let mut root = LinuxScopeRoots.open_root(path)?;
        root.directory = Box::new(Traced {
            inner: root.directory,
            trace: self.0.clone(),
        });
        Ok(root)
    }
}

/// Delegates every call to the real directory and records filter decisions and opens.
struct Traced {
    inner: Box<dyn AnchoredDirectory>,
    trace: Arc<Trace>,
}

fn record(trace: &Trace, name: &[u8], decision: FilterDecision) -> FilterDecision {
    if let Ok(mut offered) = trace.offered.lock() {
        offered.push((name.to_vec(), decision));
    }
    decision
}

impl DirectoryObservations for Traced {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.inner.next_batch()
    }

    fn next_batch_timed(&mut self, clock: Clock<'_>) -> Result<Vec<TimedObservedEntry>, PortError> {
        self.inner.next_batch_timed(clock)
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        let trace = self.trace.clone();
        self.inner
            .next_filtered(&mut |name| record(&trace, name, include(name)))
    }

    fn next_filtered_timed(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: Clock<'_>,
    ) -> Result<TimedFilteredBatch, PortError> {
        let trace = self.trace.clone();
        self.inner
            .next_filtered_timed(&mut |name| record(&trace, name, include(name)), clock)
    }
}

impl AnchoredDirectory for Traced {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        if let Ok(mut opened) = self.trace.opened.lock() {
            opened.push(entry.name.clone());
        }
        Ok(Box::new(Traced {
            inner: self.inner.open_child(entry)?,
            trace: self.trace.clone(),
        }))
    }

    fn observe_directory_case(&self, clock: Clock<'_>) -> Option<DirectoryCaseEvidence> {
        self.inner.observe_directory_case(clock)
    }

    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        self.inner.volume_space_reader()
    }

    fn observed_modified_at(&self) -> Option<Timestamp> {
        self.inner.observed_modified_at()
    }

    fn observed_modified_at_timed(
        &self,
        clock: Clock<'_>,
    ) -> (Option<Timestamp>, Option<Timestamp>) {
        self.inner.observed_modified_at_timed(clock)
    }

    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        self.inner.ancestor_file_ids()
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        self.inner.observed_identity()
    }
}
