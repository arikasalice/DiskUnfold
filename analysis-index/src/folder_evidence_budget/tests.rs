use super::{FolderEvidenceBudget, MAX_FOLDER_MANAGED_BYTES};
use std::sync::Arc;

fn base_bytes(
    budget: &FolderEvidenceBudget,
    scope: &super::FolderScopeHandle,
) -> Result<usize, String> {
    budget
        .scopes
        .lock()
        .map_err(|error| error.to_string())?
        .scopes
        .get(scope.scope_id)
        .and_then(Option::as_ref)
        .map(|entry| entry.base_bytes)
        .ok_or_else(|| "folder scope was not retained".to_owned())
}

#[test]
fn replacement_folder_scopes_share_quota_and_release_it_with_old_snapshot() -> Result<(), String> {
    let budget = Arc::new(FolderEvidenceBudget::default());
    let old_build = budget
        .folder_memory(0, 0, "cm9vdA", &[b"dir".to_vec()])
        .ok_or("scope allocation failed")?;
    let replacement = budget
        .folder_memory(0, 0, "cm9vdA", &[b"dir".to_vec()])
        .ok_or("same raw path did not reuse its scope")?;
    assert_eq!(old_build.scope_id, replacement.scope_id);

    let key_bytes = base_bytes(&budget, &old_build)?;
    let old_snapshot = budget
        .reserve_for_folder(&old_build, MAX_FOLDER_MANAGED_BYTES - key_bytes - 128)
        .ok_or("old evidence quota reservation failed")?;
    assert!(budget.reserve_for_folder(&replacement, 256).is_none());

    drop(old_snapshot);
    assert!(budget.reserve_for_folder(&replacement, 256).is_some());
    Ok(())
}

#[test]
fn raw_parent_paths_have_separate_folder_quotas() -> Result<(), String> {
    let budget = Arc::new(FolderEvidenceBudget::default());
    let left = budget
        .folder_memory(0, 0, "cm9vdA", &[b"left".to_vec()])
        .ok_or("left scope allocation failed")?;
    let right = budget
        .folder_memory(0, 0, "cm9vdA", &[b"right".to_vec()])
        .ok_or("right scope allocation failed")?;
    assert_ne!(left.scope_id, right.scope_id);
    let left_bytes = base_bytes(&budget, &left)?;
    let right_bytes = base_bytes(&budget, &right)?;
    let left_lease = budget
        .reserve_for_folder(&left, MAX_FOLDER_MANAGED_BYTES - left_bytes)
        .ok_or("left quota reservation failed")?;
    let right_lease = budget
        .reserve_for_folder(&right, MAX_FOLDER_MANAGED_BYTES - right_bytes)
        .ok_or("right quota reservation failed")?;
    drop((left_lease, right_lease));
    Ok(())
}

#[test]
fn scope_key_and_registry_leases_match_actual_vec_capacities() -> Result<(), String> {
    let budget = Arc::new(FolderEvidenceBudget::default());
    let scope = budget
        .folder_memory(3, 1, "root", &[b"parent".to_vec()])
        .ok_or("scope allocation failed")?;
    let registry = budget.scopes.lock().map_err(|error| error.to_string())?;
    let entry = registry
        .scopes
        .get(scope.scope_id)
        .and_then(Option::as_ref)
        .ok_or("scope registry entry is missing")?;
    assert_eq!(entry._key_lease.bytes(), entry.key.capacity());
    assert_eq!(entry.base_bytes, entry.key.capacity());
    let registry_lease = registry
        .capacity_lease
        .as_ref()
        .ok_or("scope registry capacity lease is missing")?;
    assert_eq!(
        registry_lease.bytes(),
        std::mem::size_of::<Option<super::FolderScope>>() * registry.scopes.capacity()
    );
    Ok(())
}

#[test]
fn overage_charge_failure_preserves_the_original_reservation() -> Result<(), String> {
    let budget = Arc::new(FolderEvidenceBudget::default());
    let scope = budget
        .folder_memory(0, 0, "root", &[b"parent".to_vec()])
        .ok_or("scope allocation failed")?;
    let mut lease = budget
        .reserve_for_folder(&scope, 64)
        .ok_or("initial folder reservation failed")?;
    let blocker = budget
        .reserve_remaining()
        .ok_or("global budget blocker failed")?;
    let used_before = budget.used_bytes();

    // Simulates a measured spare-capacity charge failing; it does not force allocator overgrowth.
    assert!(!lease.reconcile_capacity(&budget, Some(&scope), 1, 65));
    assert_eq!(lease.bytes(), 64);
    assert_eq!(budget.used_bytes(), used_before);
    drop(blocker);
    Ok(())
}
