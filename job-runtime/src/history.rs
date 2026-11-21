use du_core_contracts::{
    Id,
    job::{ItemHistoryError, ItemHistoryRecord, ItemHistoryStart, JobHistory},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct State {
    next_sequence: u64,
    closed: bool,
}

/// Serializes one running job's item sequence and storage acknowledgements.
pub(crate) struct HistoryRecorder {
    job_id: Id,
    history: Arc<dyn JobHistory>,
    state: Mutex<State>,
    failed: AtomicBool,
}

impl HistoryRecorder {
    /// Persists the start before making a recorder available to the job body.
    pub(crate) fn start(
        history: Arc<dyn JobHistory>,
        start: &ItemHistoryStart,
    ) -> Result<Arc<Self>, ItemHistoryError> {
        history.append_item_start(start)?;
        Ok(Arc::new(Self {
            job_id: start.job_id.clone(),
            history,
            state: Mutex::new(State::default()),
            failed: AtomicBool::new(false),
        }))
    }

    /// Appends the next item and advances the sequence only after storage succeeds.
    pub(crate) fn record(&self, item: ItemHistoryRecord) -> Result<(), ItemHistoryError> {
        let mut state = self.state.lock().map_err(|_| {
            self.failed.store(true, Ordering::Release);
            ItemHistoryError::Unavailable
        })?;
        if state.closed || self.failed.load(Ordering::Acquire) {
            return Err(ItemHistoryError::Unavailable);
        }
        if item.item_seq < state.next_sequence {
            return Err(ItemHistoryError::Duplicate);
        }
        if item.item_seq > state.next_sequence {
            return Err(ItemHistoryError::OutOfOrder);
        }
        let next_sequence = state
            .next_sequence
            .checked_add(1)
            .ok_or(ItemHistoryError::Unavailable)?;
        if let Err(error) = self.history.append_item_result(&self.job_id, &item) {
            self.failed.store(true, Ordering::Release);
            return Err(error);
        }
        state.next_sequence = next_sequence;
        Ok(())
    }

    /// Prevents reports from escaping the body and its registered flows.
    pub(crate) fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
        } else {
            self.failed.store(true, Ordering::Release);
        }
    }

    /// Returns whether an item append failed and made the job unable to continue.
    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}
