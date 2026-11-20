//! Builds a bounded, immutable folder-evidence plate for one result version.
use crate::{
    BuildError,
    folder_evidence_attempt::AttemptState,
    folder_evidence_budget::{FolderEvidenceBudget, FolderEvidenceLease, FolderScopeHandle},
    folder_evidence_receiver::FolderAttemptLedger,
    folder_evidence_snapshot::{FolderAttemptSummary, FolderEvidenceSnapshot, PinnedFolderEvent},
    tree::BuiltItems,
};
use std::{mem::size_of, sync::Arc};

impl FolderAttemptLedger {
    pub(crate) fn snapshot(
        &mut self,
        built: &BuiltItems,
    ) -> Result<Option<FolderEvidenceSnapshot>, BuildError> {
        if self.attempts.is_empty() {
            return Ok(None);
        }
        match self.pin_snapshot(built) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(error) => {
                self.hold_all();
                Err(error)
            }
        }
    }

    /// Charges and copies every attempt; any failure leaves the caller to hold all attempts.
    fn pin_snapshot(&self, built: &BuiltItems) -> Result<FolderEvidenceSnapshot, BuildError> {
        let mut event_count = 0usize;
        for state in &self.attempts {
            event_count = capacity(event_count.checked_add(state.events.len()))?;
        }
        let attempt_lease_bytes = capacity(
            self.attempts
                .len()
                .checked_mul(size_of::<FolderEvidenceLease>()),
        )?;
        let overhead_bytes =
            capacity(attempt_lease_bytes.checked_add(size_of::<Option<FolderEvidenceSnapshot>>()))?;
        let mut overhead_lease = capacity(self.budget.reserve(overhead_bytes))?;
        let mut attempt_leases = Vec::new();
        attempt_leases
            .try_reserve_exact(self.attempts.len())
            .map_err(|_| BuildError::CapacityExceeded)?;
        if !overhead_lease.reconcile_capacity(
            &self.budget,
            None,
            size_of::<FolderEvidenceLease>(),
            attempt_leases.capacity(),
        ) {
            return Err(BuildError::CapacityExceeded);
        }
        let attempt_capacity_bytes = capacity(
            attempt_leases
                .capacity()
                .checked_mul(size_of::<FolderEvidenceLease>()),
        )?;
        let snapshot_base_overhead = capacity(
            size_of::<Option<FolderEvidenceSnapshot>>().checked_add(attempt_capacity_bytes),
        )?;
        for state in &self.attempts {
            attempt_leases.push(self.reserve_attempt(state)?);
        }
        let mut summaries = Vec::new();
        summaries
            .try_reserve_exact(self.attempts.len())
            .map_err(|_| BuildError::CapacityExceeded)?;
        // Each attempt lease already charges one row; the snapshot lease owns only spare capacity.
        let summary_slack = capacity(snapshot_vec_slack::<FolderAttemptSummary>(
            summaries.capacity(),
            self.attempts.len(),
        ))?;
        let summary_overhead = capacity(snapshot_base_overhead.checked_add(summary_slack))?;
        reconcile_snapshot_bytes(&mut overhead_lease, &self.budget, None, summary_overhead)?;
        let mut pinned_events = Vec::new();
        pinned_events
            .try_reserve_exact(event_count)
            .map_err(|_| BuildError::CapacityExceeded)?;
        let event_slack = capacity(snapshot_vec_slack::<PinnedFolderEvent>(
            pinned_events.capacity(),
            event_count,
        ))?;
        let actual_overhead_bytes = capacity(summary_overhead.checked_add(event_slack))?;
        reconcile_snapshot_bytes(
            &mut overhead_lease,
            &self.budget,
            None,
            actual_overhead_bytes,
        )?;
        for (state, lease) in self.attempts.iter().zip(attempt_leases.iter_mut()) {
            self.pin_attempt(state, lease, built, &mut summaries, &mut pinned_events)?;
        }
        Ok(FolderEvidenceSnapshot::new(
            summaries,
            pinned_events,
            attempt_leases,
            overhead_lease,
        ))
    }

    /// Reserves one attempt's summary row, event rows, and stored payload bytes.
    fn reserve_attempt(&self, state: &AttemptState) -> Result<FolderEvidenceLease, BuildError> {
        let event_bytes = capacity(
            state
                .events
                .len()
                .checked_mul(size_of::<PinnedFolderEvent>()),
        )?;
        let payload_bytes = capacity(managed_payload_bytes(state))?;
        let folder_bytes = capacity(
            size_of::<FolderAttemptSummary>()
                .checked_add(event_bytes)
                .and_then(|bytes| bytes.checked_add(payload_bytes)),
        )?;
        capacity(self.budget.reserve_for_folder(&state.scope, folder_bytes))
    }

    /// Pins one attempt's summary and copied events, growing its lease to the copied size.
    fn pin_attempt(
        &self,
        state: &AttemptState,
        lease: &mut FolderEvidenceLease,
        built: &BuiltItems,
        summaries: &mut Vec<FolderAttemptSummary>,
        pinned_events: &mut Vec<PinnedFolderEvent>,
    ) -> Result<(), BuildError> {
        let folder_item = built
            .item_of_node
            .get(state.token.folder_node as usize)
            .ok_or(BuildError::InvalidTree)?;
        summaries.push(FolderAttemptSummary {
            attempt_id: state.token.attempt_id,
            root_index: state.token.root_index,
            folder_item: *folder_item,
            status: state.status,
            event_watermark_exclusive: state.end.map(|end| end.event_watermark_exclusive),
            child_count_exclusive: state.end.map(|end| end.child_count_exclusive),
        });
        let base_snapshot_bytes = capacity(
            state
                .events
                .len()
                .checked_mul(size_of::<PinnedFolderEvent>())
                .and_then(|event_bytes| size_of::<FolderAttemptSummary>().checked_add(event_bytes)),
        )?;
        let source_payload_bytes = capacity(managed_payload_bytes(state))?;
        let mut cloned_payload_bytes = 0usize;
        for event in &state.events {
            let input = match &event.input {
                Some(input) => {
                    let input = capacity(super::folder_evidence_copy::try_clone_input(input))?;
                    cloned_payload_bytes = capacity(
                        input
                            .managed_payload_capacity()
                            .and_then(|bytes| cloned_payload_bytes.checked_add(bytes)),
                    )?;
                    if cloned_payload_bytes > source_payload_bytes {
                        let required_bytes =
                            capacity(base_snapshot_bytes.checked_add(cloned_payload_bytes))?;
                        reconcile_snapshot_bytes(
                            lease,
                            &self.budget,
                            Some(&state.scope),
                            required_bytes,
                        )?;
                    }
                    Some(input)
                }
                None => None,
            };
            pinned_events.push(PinnedFolderEvent {
                attempt_id: state.token.attempt_id,
                _event_seq: event.event_seq,
                input,
            });
        }
        let required_bytes = capacity(base_snapshot_bytes.checked_add(cloned_payload_bytes))?;
        reconcile_snapshot_bytes(lease, &self.budget, Some(&state.scope), required_bytes)
    }
}

fn capacity<T>(value: Option<T>) -> Result<T, BuildError> {
    value.ok_or(BuildError::CapacityExceeded)
}

fn managed_payload_bytes(state: &AttemptState) -> Option<usize> {
    let mut total = 0usize;
    for event in &state.events {
        if let Some(input) = &event.input {
            total = total.checked_add(input.managed_payload_len()?)?;
        }
    }
    Some(total)
}

fn snapshot_vec_slack<T>(actual_capacity: usize, accounted_items: usize) -> Option<usize> {
    actual_capacity
        .checked_sub(accounted_items)?
        .checked_mul(size_of::<T>())
}

fn reconcile_snapshot_bytes(
    lease: &mut FolderEvidenceLease,
    budget: &Arc<FolderEvidenceBudget>,
    folder: Option<&FolderScopeHandle>,
    bytes: usize,
) -> Result<(), BuildError> {
    if lease.reconcile_bytes(budget, folder, bytes) {
        Ok(())
    } else {
        Err(BuildError::CapacityExceeded)
    }
}

#[cfg(test)]
mod tests {
    use super::{FolderAttemptSummary, reconcile_snapshot_bytes, snapshot_vec_slack};
    use crate::folder_evidence_budget::FolderEvidenceBudget;
    use std::{mem::size_of, sync::Arc};

    #[test]
    fn summary_slack_excludes_precharged_rows_and_holds_at_folder_limit() -> Result<(), String> {
        let rows = 3usize;
        let mut summaries = Vec::<FolderAttemptSummary>::new();
        summaries
            .try_reserve_exact(rows + 1)
            .map_err(|error| format!("{error:?}"))?;
        let actual_capacity = summaries.capacity();
        assert!(actual_capacity > rows);
        let slack = snapshot_vec_slack::<FolderAttemptSummary>(actual_capacity, rows)
            .ok_or("summary capacity accounting failed")?;
        assert_eq!(
            slack,
            (actual_capacity - rows) * size_of::<FolderAttemptSummary>()
        );

        let budget = Arc::new(FolderEvidenceBudget::default());
        let scope = budget
            .folder_memory(0, 0, "root", &[b"folder".to_vec()])
            .ok_or("folder scope allocation failed")?;
        let mut rows_lease = budget
            .reserve_for_folder(&scope, rows * size_of::<FolderAttemptSummary>())
            .ok_or("summary rows reservation failed")?;
        let blocker = budget
            .reserve_folder_remaining(&scope)
            .ok_or("folder limit blocker failed")?;
        let used_at_limit = budget.used_bytes();
        assert!(matches!(
            reconcile_snapshot_bytes(
                &mut rows_lease,
                &budget,
                Some(&scope),
                rows * size_of::<FolderAttemptSummary>() + slack
            ),
            Err(crate::BuildError::CapacityExceeded)
        ));
        assert_eq!(rows_lease.bytes(), rows * size_of::<FolderAttemptSummary>());
        assert_eq!(budget.used_bytes(), used_at_limit);
        drop(blocker);
        Ok(())
    }
}
