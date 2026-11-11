//! Roll back an unfinished result build and restore its own published baseline.
use crate::BuildHandle;
use du_core_contracts::result_events::ResultVersionChange;

impl BuildHandle {
    /// Aborts this build and restores its baseline only if it published a candidate.
    pub fn abort(&mut self) {
        if !self.finished {
            self.finished = true;
            let order = self
                .events
                .order
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let withdrawn = if self.reserved {
                state.abort(&self.lineage, self.baseline.clone(), self.published)
            } else {
                None
            };
            let sequence = withdrawn
                .as_ref()
                .and_then(|_| state.next_event_sequence(self.lineage.as_ref()));
            drop(state);
            self.events.notify(
                self.lineage.as_ref(),
                sequence,
                self.origin.as_str(),
                ResultVersionChange::Withdrawn,
                withdrawn.as_ref().map(|entry| entry.result.as_ref()),
                self.baseline.as_ref().map(|entry| entry.result.as_ref()),
            );
            drop(order);
            drop(withdrawn);
        }
    }
}

impl Drop for BuildHandle {
    fn drop(&mut self) {
        self.abort();
    }
}
