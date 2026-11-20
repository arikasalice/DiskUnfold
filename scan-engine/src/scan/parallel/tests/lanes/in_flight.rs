use super::Probe;
use std::sync::atomic::Ordering;

/// Counts one synthetic OS call as in flight until it returns.
pub(super) struct InFlight<'a>(&'a Probe);

impl<'a> InFlight<'a> {
    pub(super) fn enter(probe: &'a Probe) -> Self {
        let active = probe.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        probe.in_flight_peak.fetch_max(active, Ordering::SeqCst);
        Self(probe)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}
