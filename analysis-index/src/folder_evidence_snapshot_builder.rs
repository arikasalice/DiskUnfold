//! Builds a bounded, immutable folder-evidence plate for one result version.
use crate::{
    BuildError,
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
        let mut event_count = 0usize;
        for state in &self.attempts {
            let Some(next_event_count) = event_count.checked_add(state.events.len()) else {
                self.hold_all();
                return Err(BuildError::CapacityExceeded);
            };
            event_count = next_event_count;
        }
        let Some(attempt_lease_bytes) = self
            .attempts
            .len()
            .checked_mul(size_of::<FolderEvidenceLease>())
        else {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        };
        let Some(overhead_bytes) =
            attempt_lease_bytes.checked_add(size_of::<Option<FolderEvidenceSnapshot>>())
        else {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        };
        let Some(mut overhead_lease) = self.budget.reserve(overhead_bytes) else {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        };
        let mut attempt_leases = Vec::new();
        if attempt_leases
            .try_reserve_exact(self.attempts.len())
            .is_err()
        {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        }
        if !overhead_lease.reconcile_capacity(
            &self.budget,
            None,
            size_of::<FolderEvidenceLease>(),
            attempt_leases.capacity(),
        ) {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        }
        let Some(attempt_capacity_bytes) = attempt_leases
            .capacity()
            .checked_mul(size_of::<FolderEvidenceLease>())
        else {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        };
        let Some(snapshot_base_overhead) =
            size_of::<Option<FolderEvidenceSnapshot>>().checked_add(attempt_capacity_bytes)
        else {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        };
        let mut attempt_leases_valid = true;
        'attempts: for state in &self.attempts {
            let Some(event_bytes) = state
                .events
                .len()
                .checked_mul(size_of::<PinnedFolderEvent>())
            else {
                attempt_leases_valid = false;
                break;
            };
            let mut payload_bytes = 0usize;
            for event in &state.events {
                let Some(input) = &event.input else {
                    continue;
                };
                let Some(bytes) = input.managed_payload_len() else {
                    attempt_leases_valid = false;
                    break 'attempts;
                };
                let Some(next_bytes) = payload_bytes.checked_add(bytes) else {
                    attempt_leases_valid = false;
                    break 'attempts;
                };
                payload_bytes = next_bytes;
            }
            let Some(folder_bytes) = size_of::<FolderAttemptSummary>()
                .checked_add(event_bytes)
                .and_then(|bytes| bytes.checked_add(payload_bytes))
            else {
                attempt_leases_valid = false;
                break;
            };
            let Some(lease) = self.budget.reserve_for_folder(&state.scope, folder_bytes) else {
                attempt_leases_valid = false;
                break;
            };
            attempt_leases.push(lease);
        }
        if !attempt_leases_valid {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        }
        let mut summaries = Vec::new();
        if summaries.try_reserve_exact(self.attempts.len()).is_err() {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        }
        // Each attempt lease already charges one row; the snapshot lease owns only spare capacity.
        let Some(summary_slack) =
            snapshot_vec_slack::<FolderAttemptSummary>(summaries.capacity(), self.attempts.len())
        else {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        };
        let Some(summary_overhead) = snapshot_base_overhead.checked_add(summary_slack) else {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        };
        if reconcile_snapshot_bytes(&mut overhead_lease, &self.budget, None, summary_overhead)
            .is_err()
        {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        }
        let mut pinned_events = Vec::new();
        if pinned_events.try_reserve_exact(event_count).is_err() {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        }
        let Some(event_slack) =
            snapshot_vec_slack::<PinnedFolderEvent>(pinned_events.capacity(), event_count)
        else {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        };
        let Some(actual_overhead_bytes) = summary_overhead.checked_add(event_slack) else {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        };
        if reconcile_snapshot_bytes(
            &mut overhead_lease,
            &self.budget,
            None,
            actual_overhead_bytes,
        )
        .is_err()
        {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        }
        let mut binding_valid = true;
        let mut allocation_failed = false;
        for (attempt_index, state) in self.attempts.iter().enumerate() {
            let Some(folder_item) = built.item_of_node.get(state.token.folder_node as usize) else {
                binding_valid = false;
                break;
            };
            summaries.push(FolderAttemptSummary {
                attempt_id: state.token.attempt_id,
                root_index: state.token.root_index,
                folder_item: *folder_item,
                status: state.status,
                event_watermark_exclusive: state.end.map(|end| end.event_watermark_exclusive),
                child_count_exclusive: state.end.map(|end| end.child_count_exclusive),
            });
            let base_snapshot_bytes = match state
                .events
                .len()
                .checked_mul(size_of::<PinnedFolderEvent>())
                .and_then(|event_bytes| size_of::<FolderAttemptSummary>().checked_add(event_bytes))
            {
                Some(bytes) => bytes,
                None => {
                    allocation_failed = true;
                    break;
                }
            };
            let mut source_payload_bytes = 0usize;
            for event in &state.events {
                let Some(input) = event.input.as_ref() else {
                    continue;
                };
                let Some(bytes) = input.managed_payload_len() else {
                    allocation_failed = true;
                    break;
                };
                let Some(next) = source_payload_bytes.checked_add(bytes) else {
                    allocation_failed = true;
                    break;
                };
                source_payload_bytes = next;
            }
            if allocation_failed {
                break;
            }
            let mut cloned_payload_bytes = 0usize;
            for event in &state.events {
                let input = match &event.input {
                    Some(input) => match super::folder_evidence_copy::try_clone_input(input) {
                        Some(input) => {
                            let Some(bytes) = input.managed_payload_capacity() else {
                                allocation_failed = true;
                                break;
                            };
                            let Some(next) = cloned_payload_bytes.checked_add(bytes) else {
                                allocation_failed = true;
                                break;
                            };
                            cloned_payload_bytes = next;
                            if cloned_payload_bytes > source_payload_bytes {
                                let Some(required_bytes) =
                                    base_snapshot_bytes.checked_add(cloned_payload_bytes)
                                else {
                                    allocation_failed = true;
                                    break;
                                };
                                let Some(lease) = attempt_leases.get_mut(attempt_index) else {
                                    allocation_failed = true;
                                    break;
                                };
                                if reconcile_snapshot_bytes(
                                    lease,
                                    &self.budget,
                                    Some(&state.scope),
                                    required_bytes,
                                )
                                .is_err()
                                {
                                    allocation_failed = true;
                                    break;
                                }
                            }
                            Some(input)
                        }
                        None => {
                            allocation_failed = true;
                            break;
                        }
                    },
                    None => None,
                };
                pinned_events.push(PinnedFolderEvent {
                    _attempt_id: state.token.attempt_id,
                    _event_seq: event.event_seq,
                    _input: input,
                });
            }
            if !allocation_failed {
                let Some(required_bytes) = base_snapshot_bytes.checked_add(cloned_payload_bytes)
                else {
                    allocation_failed = true;
                    break;
                };
                let Some(lease) = attempt_leases.get_mut(attempt_index) else {
                    allocation_failed = true;
                    break;
                };
                if reconcile_snapshot_bytes(lease, &self.budget, Some(&state.scope), required_bytes)
                    .is_err()
                {
                    allocation_failed = true;
                    break;
                }
            }
            if !binding_valid || allocation_failed {
                break;
            }
        }
        if allocation_failed {
            self.hold_all();
            return Err(BuildError::CapacityExceeded);
        }
        if !binding_valid {
            self.hold_all();
            return Err(BuildError::InvalidTree);
        }
        Ok(Some(FolderEvidenceSnapshot::new(
            summaries,
            pinned_events,
            attempt_leases,
            overhead_lease,
        )))
    }
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
