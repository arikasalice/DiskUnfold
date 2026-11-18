use super::super::{
    AttemptState, FolderAttemptLedger, StoredAttemptEvent, registry_growth_capacity,
    try_grow_attempt_slots,
};
use crate::folder_evidence_budget::{FolderEvidenceBudget, MAX_BUILD_MANAGED_BYTES};
use std::{mem::size_of, sync::Arc};

fn fill_registry_to_capacity()
-> Result<(Arc<FolderEvidenceBudget>, FolderAttemptLedger, Vec<u8>), String> {
    let budget = Arc::new(FolderEvidenceBudget::default());
    let mut ledger = FolderAttemptLedger::new(1, Arc::clone(&budget));
    while ledger.attempts.is_empty() || ledger.attempts.len() < ledger.attempts.capacity() {
        let attempt_id = u64::try_from(ledger.attempts.len()).map_err(|error| error.to_string())?;
        let path = [attempt_id.to_le_bytes().to_vec()];
        ledger
            .begin(0, 0, attempt_id, 0, "root", &path)
            .map_err(|error| format!("{error:?}"))?;
    }
    Ok((budget, ledger, 0u64.to_le_bytes().to_vec()))
}

#[test]
fn registry_growth_policy_doubles_requested_capacity_with_a_limit() {
    assert_eq!(registry_growth_capacity(0, 1), Some(8));
    assert_eq!(registry_growth_capacity(8, 9), Some(16));
    assert_eq!(registry_growth_capacity(16, 17), Some(32));
    assert_eq!(registry_growth_capacity(2048, 2049), Some(4096));
    assert_eq!(registry_growth_capacity(4096, 4097), None);
}

#[test]
fn registry_lease_tracks_actual_capacity_across_growth() -> Result<(), String> {
    let (budget, mut ledger, first_path) = fill_registry_to_capacity()?;
    let old_capacity = ledger.attempts.capacity();
    let old_bytes = ledger
        .registry_lease
        .as_ref()
        .ok_or("missing old registry lease")?
        .bytes();
    let next_id = u64::try_from(ledger.attempts.len()).map_err(|error| error.to_string())?;
    let path = [first_path];
    let used_before = budget.used_bytes();
    ledger
        .begin(0, 0, next_id, 0, "root", &path)
        .map_err(|error| format!("{error:?}"))?;
    let new_bytes = ledger
        .registry_lease
        .as_ref()
        .ok_or("missing replacement registry lease")?
        .bytes();
    let event_bytes = ledger
        .attempts
        .last()
        .and_then(|attempt| attempt.event_capacity_lease.as_ref())
        .map(crate::folder_evidence_budget::FolderEvidenceLease::bytes)
        .ok_or("missing initial event lease")?;

    assert_eq!(ledger.attempts.len(), old_capacity + 1);
    assert_eq!(
        new_bytes,
        size_of::<AttemptState>() * ledger.attempts.capacity()
    );
    assert_eq!(
        event_bytes,
        size_of::<StoredAttemptEvent>() * ledger.attempts[old_capacity].events.capacity()
    );
    assert!(new_bytes > old_bytes);
    assert_eq!(
        budget.used_bytes(),
        used_before + event_bytes + new_bytes - old_bytes
    );
    Ok(())
}

#[test]
fn registry_resize_reservation_failure_preserves_old_buffer_and_lease() -> Result<(), String> {
    let (budget, mut ledger, _) = fill_registry_to_capacity()?;
    let old_capacity = ledger.attempts.capacity();
    let old_len = ledger.attempts.len();
    let old_bytes = ledger
        .registry_lease
        .as_ref()
        .ok_or("missing old registry lease")?
        .bytes();
    let target = registry_growth_capacity(old_capacity, old_capacity + 1)
        .ok_or("no registry growth target")?;
    let registry_bytes = size_of::<AttemptState>()
        .checked_mul(target)
        .ok_or("registry size overflow")?;
    let leave = registry_bytes.checked_sub(1).ok_or("empty registry size")?;
    let used = budget.used_bytes();
    let blocker_bytes = MAX_BUILD_MANAGED_BYTES
        .checked_sub(used)
        .and_then(|remaining| remaining.checked_sub(leave))
        .ok_or("test setup exceeds budget")?;
    let blocker = budget
        .reserve(blocker_bytes)
        .ok_or("failed to reserve test blocker")?;
    let before_growth = budget.used_bytes();

    assert!(try_grow_attempt_slots(&mut ledger.attempts, target, &budget).is_none());
    assert_eq!(ledger.attempts.len(), old_len);
    assert_eq!(ledger.attempts.capacity(), old_capacity);
    assert_eq!(
        ledger
            .registry_lease
            .as_ref()
            .ok_or("old registry lease was lost")?
            .bytes(),
        old_bytes
    );
    assert_eq!(budget.used_bytes(), before_growth);
    drop(blocker);
    Ok(())
}

#[test]
fn capacity_overflow_preserves_existing_attempts() -> Result<(), String> {
    let (budget, mut ledger, _) = fill_registry_to_capacity()?;
    let old_capacity = ledger.attempts.capacity();
    let old_len = ledger.attempts.len();
    assert!(try_grow_attempt_slots(&mut ledger.attempts, usize::MAX, &budget).is_none());
    assert_eq!(ledger.attempts.len(), old_len);
    assert_eq!(ledger.attempts.capacity(), old_capacity);
    Ok(())
}
