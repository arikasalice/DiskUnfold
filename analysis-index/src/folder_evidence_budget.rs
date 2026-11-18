//! Shared managed-memory budget for active attempt ledgers and immutable version summaries.
//!
//! Per-`IngestSink` accounting candidate for managed folder-evidence capacities. Vec capacities
//! are reconciled after allocation; an allocator can temporarily return spare capacity before
//! that excess is charged or the allocation is discarded. This is not a strict peak cap, allocator
//! metadata, or process-RSS limit; a fixed arena or bounded allocator is required for that.
use std::{
    mem::size_of,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

// Clones of one IngestSink share this budget. Independently constructed sinks have
// separate budgets, so these constants do not impose a process-wide limit.
pub(crate) const MAX_FOLDER_MANAGED_BYTES: usize = 1 << 20;
pub(crate) const MAX_BUILD_MANAGED_BYTES: usize = 32 << 20;

#[derive(Debug, Default)]
pub(crate) struct FolderEvidenceBudget {
    used: AtomicUsize,
    scopes: Mutex<FolderScopeRegistry>,
}

#[derive(Debug)]
pub(crate) struct FolderEvidenceLease {
    budget: Weak<FolderEvidenceBudget>,
    scope_id: Option<usize>,
    bytes: usize,
}

#[derive(Debug, Default)]
struct FolderScopeRegistry {
    scopes: Vec<Option<FolderScope>>,
    capacity_lease: Option<FolderEvidenceLease>,
}

#[derive(Debug)]
pub(crate) struct FolderScopeHandle {
    budget: Weak<FolderEvidenceBudget>,
    scope_id: usize,
}

#[derive(Debug)]
struct FolderScope {
    key: Vec<u8>,
    used: usize,
    base_bytes: usize,
    pins: usize,
    _key_lease: FolderEvidenceLease,
}

impl FolderEvidenceBudget {
    pub(crate) fn reserve(self: &Arc<Self>, bytes: usize) -> Option<FolderEvidenceLease> {
        let mut current = self.used.load(Ordering::Relaxed);
        loop {
            let next = current.checked_add(bytes)?;
            if next > MAX_BUILD_MANAGED_BYTES {
                return None;
            }
            match self.used.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Some(FolderEvidenceLease {
                        budget: Arc::downgrade(self),
                        scope_id: None,
                        bytes,
                    });
                }
                Err(actual) => current = actual,
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn reserve_remaining(self: &Arc<Self>) -> Option<FolderEvidenceLease> {
        let used = self.used.load(Ordering::Relaxed);
        self.reserve(MAX_BUILD_MANAGED_BYTES.checked_sub(used)?)
    }

    #[cfg(test)]
    pub(crate) fn used_bytes(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    pub(crate) fn reserve_for_folder(
        self: &Arc<Self>,
        folder: &FolderScopeHandle,
        bytes: usize,
    ) -> Option<FolderEvidenceLease> {
        let mut lease = self.reserve(bytes)?;
        if !Weak::ptr_eq(&folder.budget, &Arc::downgrade(self)) {
            return None;
        }
        let mut registry = self.scopes.lock().ok()?;
        let scope = registry.scopes.get_mut(folder.scope_id)?.as_mut()?;
        let next = scope.used.checked_add(bytes)?;
        if next > MAX_FOLDER_MANAGED_BYTES {
            return None;
        }
        scope.used = next;
        lease.scope_id = Some(folder.scope_id);
        Some(lease)
    }

    fn grow_scope_registry(
        self: &Arc<Self>,
        registry: &mut FolderScopeRegistry,
        target_capacity: usize,
    ) -> Option<FolderEvidenceLease> {
        let requested_bytes = target_capacity.checked_mul(size_of::<Option<FolderScope>>())?;
        let mut lease = self.reserve(requested_bytes)?;
        let mut grown = Vec::new();
        grown.try_reserve_exact(target_capacity).ok()?;
        if !lease.reconcile_capacity(
            self,
            None,
            size_of::<Option<FolderScope>>(),
            grown.capacity(),
        ) {
            return None;
        }

        let mut previous = std::mem::take(&mut registry.scopes);
        grown.append(&mut previous);
        registry.scopes = grown;
        drop(previous);
        Some(lease)
    }

    #[cfg(test)]
    pub(crate) fn reserve_folder_remaining(
        self: &Arc<Self>,
        folder: &FolderScopeHandle,
    ) -> Option<FolderEvidenceLease> {
        let remaining = {
            let registry = self.scopes.lock().ok()?;
            let scope = registry.scopes.get(folder.scope_id)?.as_ref()?;
            MAX_FOLDER_MANAGED_BYTES.checked_sub(scope.used)?
        };
        self.reserve_for_folder(folder, remaining)
    }

    /// Returns the shared quota for one root-index and exact raw-parent path.
    pub(crate) fn folder_memory(
        self: &Arc<Self>,
        root_index: u32,
        root_encoding: u8,
        root_path: &str,
        parent_path: &[Vec<u8>],
    ) -> Option<FolderScopeHandle> {
        let mut registry = self.scopes.lock().ok()?;
        if let Some((scope_id, scope)) =
            registry
                .scopes
                .iter_mut()
                .enumerate()
                .find_map(|(scope_id, slot)| {
                    let scope = slot.as_mut()?;
                    scope_key_matches(
                        &scope.key,
                        root_index,
                        root_encoding,
                        root_path,
                        parent_path,
                    )
                    .then_some((scope_id, scope))
                })
        {
            scope.pins = scope.pins.checked_add(1)?;
            return Some(FolderScopeHandle {
                budget: Arc::downgrade(self),
                scope_id,
            });
        }

        for slot in &mut registry.scopes {
            if slot
                .as_ref()
                .is_some_and(|scope| scope.used == scope.base_bytes && scope.pins == 0)
            {
                *slot = None;
            }
        }

        let key_len = parent_path.iter().try_fold(
            4usize
                .checked_add(1 + 8)?
                .checked_add(root_path.len())?
                .checked_add(8)?,
            |length, component| length.checked_add(8)?.checked_add(component.len()),
        )?;
        if key_len > MAX_FOLDER_MANAGED_BYTES {
            return None;
        }
        let mut key_lease = self.reserve(key_len)?;
        let mut key = Vec::new();
        key.try_reserve_exact(key_len).ok()?;
        if !key_lease.reconcile_capacity(self, None, size_of::<u8>(), key.capacity())
            || key.capacity() > MAX_FOLDER_MANAGED_BYTES
        {
            return None;
        }
        key.extend_from_slice(&root_index.to_le_bytes());
        key.push(root_encoding);
        append_field(&mut key, root_path.as_bytes())?;
        key.extend_from_slice(&u64::try_from(parent_path.len()).ok()?.to_le_bytes());
        for component in parent_path {
            append_field(&mut key, component)?;
        }
        let key_capacity = key.capacity();

        let slot_id = match registry.scopes.iter().position(|slot| slot.is_none()) {
            Some(slot_id) => slot_id,
            None => {
                let required = registry.scopes.len().checked_add(1)?;
                if required > registry.scopes.capacity() {
                    let target = registry
                        .scopes
                        .capacity()
                        .saturating_mul(2)
                        .max(16)
                        .max(required);
                    let next_capacity_lease = self.grow_scope_registry(&mut registry, target)?;
                    registry.capacity_lease = Some(next_capacity_lease);
                }
                registry.scopes.push(None);
                registry.scopes.len().checked_sub(1)?
            }
        };

        registry.scopes[slot_id] = Some(FolderScope {
            key,
            used: key_capacity,
            base_bytes: key_capacity,
            pins: 1,
            _key_lease: key_lease,
        });
        Some(FolderScopeHandle {
            budget: Arc::downgrade(self),
            scope_id: slot_id,
        })
    }

    fn release_scope(&self, scope_id: usize, bytes: usize, unpin: bool) -> bool {
        let Ok(mut registry) = self.scopes.lock() else {
            return false;
        };
        let Some(scope) = registry.scopes.get_mut(scope_id).and_then(Option::as_mut) else {
            return false;
        };
        let unpin_count = if unpin { 1 } else { 0 };
        let Some(pins) = scope.pins.checked_sub(unpin_count) else {
            return false;
        };
        let Some(used) = scope.used.checked_sub(bytes) else {
            return false;
        };
        scope.pins = pins;
        scope.used = used;
        true
    }
}

fn append_field(output: &mut Vec<u8>, value: &[u8]) -> Option<()> {
    output.extend_from_slice(&u64::try_from(value.len()).ok()?.to_le_bytes());
    output.extend_from_slice(value);
    Some(())
}

fn scope_key_matches(
    key: &[u8],
    root_index: u32,
    root_encoding: u8,
    root_path: &str,
    parent_path: &[Vec<u8>],
) -> bool {
    let mut offset = 0usize;
    if take(key, &mut offset, &root_index.to_le_bytes()) != Some(true)
        || key.get(offset).copied() != Some(root_encoding)
    {
        return false;
    }
    offset = match offset.checked_add(1) {
        Some(offset) => offset,
        None => return false,
    };
    if take_field(key, &mut offset) != Some(root_path.as_bytes())
        || take_u64(key, &mut offset) != Some(parent_path.len() as u64)
    {
        return false;
    }
    parent_path
        .iter()
        .all(|component| take_field(key, &mut offset) == Some(component.as_slice()))
        && offset == key.len()
}

fn take(key: &[u8], offset: &mut usize, expected: &[u8]) -> Option<bool> {
    let end = offset.checked_add(expected.len())?;
    let matches = key.get(*offset..end)? == expected;
    *offset = end;
    Some(matches)
}

fn take_u64(key: &[u8], offset: &mut usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let bytes: [u8; 8] = key.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(u64::from_le_bytes(bytes))
}

fn take_field<'a>(key: &'a [u8], offset: &mut usize) -> Option<&'a [u8]> {
    let length = usize::try_from(take_u64(key, offset)?).ok()?;
    let end = offset.checked_add(length)?;
    let field = key.get(*offset..end)?;
    *offset = end;
    Some(field)
}

impl FolderEvidenceLease {
    /// Reconciles a reservation with `Vec::capacity()` after allocation. A failed overage
    /// charge requires the caller to discard the new allocation; it cannot undo the transient.
    pub(crate) fn reconcile_capacity(
        &mut self,
        budget: &Arc<FolderEvidenceBudget>,
        folder: Option<&FolderScopeHandle>,
        element_size: usize,
        capacity: usize,
    ) -> bool {
        let Some(bytes) = element_size.checked_mul(capacity) else {
            return false;
        };
        self.reconcile_bytes(budget, folder, bytes)
    }

    pub(crate) fn reconcile_bytes(
        &mut self,
        budget: &Arc<FolderEvidenceBudget>,
        folder: Option<&FolderScopeHandle>,
        bytes: usize,
    ) -> bool {
        let expected_scope = folder.map(|scope| scope.scope_id);
        if self.scope_id != expected_scope || !Weak::ptr_eq(&self.budget, &Arc::downgrade(budget)) {
            return false;
        }
        if bytes < self.bytes {
            return self.release(self.bytes - bytes);
        }
        if bytes > self.bytes {
            let extra_bytes = bytes - self.bytes;
            let Some(mut extra) = (match folder {
                Some(scope) => budget.reserve_for_folder(scope, extra_bytes),
                None => budget.reserve(extra_bytes),
            }) else {
                return false;
            };
            self.bytes = bytes;
            extra.bytes = 0;
        }
        true
    }

    fn release(&mut self, bytes: usize) -> bool {
        if bytes > self.bytes {
            return false;
        }
        if bytes == 0 {
            return true;
        }
        if let Some(budget) = self.budget.upgrade() {
            let released = self
                .scope_id
                .is_none_or(|scope_id| budget.release_scope(scope_id, bytes, false));
            if !released {
                return false;
            }
            budget.used.fetch_sub(bytes, Ordering::AcqRel);
        }
        self.bytes -= bytes;
        true
    }
}

impl Drop for FolderScopeHandle {
    fn drop(&mut self) {
        if let Some(budget) = self.budget.upgrade() {
            let _ = budget.release_scope(self.scope_id, 0, true);
        }
    }
}

impl FolderEvidenceLease {
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests;

impl Drop for FolderEvidenceLease {
    fn drop(&mut self) {
        let bytes = self.bytes;
        let _ = self.release(bytes);
    }
}
