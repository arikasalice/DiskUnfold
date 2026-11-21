//! Track session-only undo information and removal reservations.

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use du_core_contracts::{
    Id,
    job::{SessionUndoInfo, UndoInfoUnavailable},
};

#[derive(Default)]
pub(super) struct SessionUndoInfoRegistry {
    entries: Mutex<HashMap<Id, Entry>>,
}

#[derive(Default)]
struct Entry {
    active_uses: u32,
    reserved: bool,
}

impl SessionUndoInfoRegistry {
    /// Registers undo information produced by a session operation.
    #[allow(dead_code)]
    pub(super) fn register(&self, job_id: Id) {
        self.lock_recover().entry(job_id).or_default();
    }

    /// Starts restore use unless a retention operation has reserved the entry.
    #[allow(dead_code)]
    pub(super) fn begin_use(&self, job_id: &Id) -> Result<bool, UndoInfoUnavailable> {
        let mut entries = self.entries.lock().map_err(|_| UndoInfoUnavailable)?;
        let Some(entry) = entries.get_mut(job_id) else {
            return Ok(false);
        };
        if entry.reserved {
            return Ok(false);
        }
        entry.active_uses = entry
            .active_uses
            .checked_add(1)
            .ok_or(UndoInfoUnavailable)?;
        Ok(true)
    }

    /// Ends one restore use. Repeated calls after removal have no effect.
    #[allow(dead_code)]
    pub(super) fn end_use(&self, job_id: &Id) {
        if let Some(entry) = self.lock_recover().get_mut(job_id) {
            entry.active_uses = entry.active_uses.saturating_sub(1);
        }
    }

    fn lock_recover(&self) -> MutexGuard<'_, HashMap<Id, Entry>> {
        match self.entries.lock() {
            Ok(entries) => entries,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl SessionUndoInfo for SessionUndoInfoRegistry {
    fn present(&self, job_ids: &[Id]) -> Result<Vec<Id>, UndoInfoUnavailable> {
        let entries = self.entries.lock().map_err(|_| UndoInfoUnavailable)?;
        Ok(job_ids
            .iter()
            .filter(|job_id| entries.contains_key(*job_id))
            .cloned()
            .collect())
    }

    fn in_use(&self, job_id: &Id) -> Result<bool, UndoInfoUnavailable> {
        let entries = self.entries.lock().map_err(|_| UndoInfoUnavailable)?;
        Ok(entries
            .get(job_id)
            .is_some_and(|entry| entry.active_uses > 0))
    }

    fn reserve_removal(&self, job_ids: &[Id]) -> Vec<Id> {
        let mut entries = self.lock_recover();
        let mut reserved = Vec::new();
        for job_id in job_ids {
            if let Some(entry) = entries.get_mut(job_id)
                && !entry.reserved
            {
                entry.reserved = true;
                reserved.push(job_id.clone());
            }
        }
        reserved
    }

    fn release(&self, job_ids: &[Id]) {
        let mut entries = self.lock_recover();
        for job_id in job_ids {
            if let Some(entry) = entries.get_mut(job_id) {
                entry.reserved = false;
            }
        }
    }

    fn remove(&self, job_ids: &[Id]) {
        let mut entries = self.lock_recover();
        for job_id in job_ids {
            if entries
                .get(job_id)
                .is_some_and(|entry| entry.reserved && entry.active_uses == 0)
            {
                entries.remove(job_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(seed: u8) -> Id {
        let mut bytes = [seed; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        Id::from_v7_bytes(bytes)
    }

    #[test]
    fn empty_registry_reports_no_undo_information() -> Result<(), UndoInfoUnavailable> {
        let registry = SessionUndoInfoRegistry::default();
        let missing = id(1);
        assert!(registry.present(std::slice::from_ref(&missing))?.is_empty());
        assert!(!registry.in_use(&missing)?);
        assert!(
            registry
                .reserve_removal(std::slice::from_ref(&missing))
                .is_empty()
        );
        registry.release(std::slice::from_ref(&missing));
        registry.remove(std::slice::from_ref(&missing));
        Ok(())
    }

    #[test]
    fn reserve_blocks_new_use_then_release_and_remove_are_idempotent()
    -> Result<(), UndoInfoUnavailable> {
        let registry = SessionUndoInfoRegistry::default();
        let job_id = id(2);
        registry.register(job_id.clone());
        let reserved = registry.reserve_removal(std::slice::from_ref(&job_id));
        assert_eq!(reserved.len(), 1);
        assert_eq!(reserved.first(), Some(&job_id));
        assert_eq!(registry.present(std::slice::from_ref(&job_id))?.len(), 1);
        assert!(!registry.begin_use(&job_id)?);
        assert!(!registry.in_use(&job_id)?);

        registry.release(&reserved);
        registry.release(&reserved);
        assert!(registry.begin_use(&job_id)?);
        assert!(registry.in_use(&job_id)?);
        registry.end_use(&job_id);
        let reserved = registry.reserve_removal(std::slice::from_ref(&job_id));
        assert!(!registry.in_use(&job_id)?);
        registry.remove(&reserved);
        registry.remove(&reserved);
        assert!(registry.present(std::slice::from_ref(&job_id))?.is_empty());
        Ok(())
    }
}
