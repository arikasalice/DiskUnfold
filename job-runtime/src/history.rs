use du_core_contracts::{
    Id,
    history_ipc::HistoryItemOutcome,
    job::{
        ITEM_HISTORY_MAX_ITEM_REF_BYTES, ITEM_HISTORY_MAX_LOCATION_TEXT_CHARS, ItemHistoryError,
        ItemHistoryRecord, ItemHistoryStart, JobHistory, ObservedSize,
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct State {
    next_sequence: u64,
    last_record: Option<ItemHistoryRecord>,
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

    /// Appends the next item and advances only after storage succeeds.
    pub(crate) fn record(&self, item: ItemHistoryRecord) -> Result<(), ItemHistoryError> {
        validate_item(&item)?;
        let mut state = self.state.lock().map_err(|_| {
            self.failed.store(true, Ordering::Release);
            ItemHistoryError::Unavailable
        })?;
        if state.closed || self.failed.load(Ordering::Acquire) {
            return Err(ItemHistoryError::Unavailable);
        }
        if item.item_seq < state.next_sequence {
            if state
                .last_record
                .as_ref()
                .is_some_and(|last| last.item_seq == item.item_seq && last == &item)
            {
                return Ok(());
            }
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
            if matches!(
                error,
                ItemHistoryError::Busy | ItemHistoryError::Unavailable
            ) {
                self.failed.store(true, Ordering::Release);
            }
            return Err(error);
        }
        state.next_sequence = next_sequence;
        state.last_record = Some(item);
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

fn validate_item(item: &ItemHistoryRecord) -> Result<(), ItemHistoryError> {
    if item.item_ref.is_empty()
        || item.item_ref.len() > ITEM_HISTORY_MAX_ITEM_REF_BYTES
        || item.item_seq > i64::MAX as u64
        || item.location_text.chars().count() > ITEM_HISTORY_MAX_LOCATION_TEXT_CHARS
        || item.displaced.as_ref().is_some_and(|displaced| {
            displaced.name.as_deref() == Some("")
                || displaced.original_location.as_deref() == Some("")
        })
        || (item.outcome == HistoryItemOutcome::Overwritten && item.displaced.is_none())
        || (item.displaced.is_some()
            && !matches!(
                item.outcome,
                HistoryItemOutcome::Overwritten | HistoryItemOutcome::Failed
            ))
        || matches!(
            item.observed_size,
            ObservedSize::Exact(size) | ObservedSize::LowerBound(size) if size > i64::MAX as u64
        )
    {
        return Err(ItemHistoryError::Invalid);
    }
    Ok(())
}
