//! Track session-only undo information and removal reservations.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, MutexGuard},
};

use du_core_contracts::{
    Id,
    job::{SessionUndoInfo, UndoInfoUnavailable},
    platform_os_change::{SessionUndoStore, TrashedUndoInfo},
    ports::{PortError, PortErrorKind},
};

const MAX_TRASHED_ITEMS: usize = 100_000;

#[derive(Default)]
pub(super) struct SessionUndoInfoRegistry {
    entries: Mutex<HashMap<Id, Entry>>,
}

#[derive(Default)]
struct Entry {
    active_uses: u32,
    reserved: bool,
    trashed: std::collections::BTreeMap<u32, TrashedUndoInfo>,
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

impl SessionUndoStore for SessionUndoInfoRegistry {
    fn register_trashed(&self, job_ref: &Id, items: &[TrashedUndoInfo]) -> Result<(), PortError> {
        let mut entries = self.lock_recover();
        if entries.get(job_ref).is_some_and(|entry| entry.reserved) {
            return Err(undo_error(
                PortErrorKind::Busy,
                "change.undo_registry.reserved",
            ));
        }
        let existing = entries.get(job_ref);
        let mut batch = BTreeMap::<u32, &TrashedUndoInfo>::new();
        for item in items {
            if let Some(current) = batch.get(&item.item_no)
                && *current != item
            {
                return Err(undo_error(
                    PortErrorKind::Corrupt,
                    "change.undo_registry.conflict",
                ));
            }
            if let Some(current) = existing.and_then(|entry| entry.trashed.get(&item.item_no))
                && current != item
            {
                return Err(undo_error(
                    PortErrorKind::Corrupt,
                    "change.undo_registry.conflict",
                ));
            }
            batch.entry(item.item_no).or_insert(item);
        }
        let new_count = batch
            .keys()
            .filter(|&&item_no| existing.is_none_or(|entry| !entry.trashed.contains_key(&item_no)))
            .count();
        let total = entries
            .values()
            .fold(0_usize, |count, entry| {
                count.saturating_add(entry.trashed.len())
            })
            .saturating_add(new_count);
        if total > MAX_TRASHED_ITEMS {
            return Err(undo_error(
                PortErrorKind::Busy,
                "change.undo_registry.limit_reached",
            ));
        }
        let entry = entries.entry(job_ref.clone()).or_default();
        for (item_no, item) in batch {
            entry.trashed.entry(item_no).or_insert_with(|| item.clone());
        }
        Ok(())
    }

    fn contains(&self, job_ref: &Id, item_no: u32) -> Result<bool, PortError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| undo_error(PortErrorKind::Io, "change.undo_registry.unavailable"))?;
        Ok(entries.get(job_ref).is_some_and(|entry| {
            !entry.reserved
                && entry
                    .trashed
                    .get(&item_no)
                    .is_some_and(|item| item.trash_location.is_some())
        }))
    }

    fn get_trashed(
        &self,
        job_ref: &Id,
        item_no: u32,
    ) -> Result<Option<TrashedUndoInfo>, PortError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| undo_error(PortErrorKind::Io, "change.undo_registry.unavailable"))?;
        Ok(entries.get(job_ref).and_then(|entry| {
            (!entry.reserved)
                .then(|| entry.trashed.get(&item_no))
                .flatten()
                .filter(|item| item.trash_location.is_some())
                .cloned()
        }))
    }

    fn remove_trashed(&self, job_ref: &Id, item_no: u32) -> Result<(), PortError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| undo_error(PortErrorKind::Io, "change.undo_registry.unavailable"))?;
        let entry = entries
            .get_mut(job_ref)
            .ok_or_else(|| undo_error(PortErrorKind::NotFound, "change.undo_registry.not_found"))?;
        if entry.reserved {
            return Err(undo_error(
                PortErrorKind::Busy,
                "change.undo_registry.reserved",
            ));
        }
        entry
            .trashed
            .remove(&item_no)
            .map(|_| ())
            .ok_or_else(|| undo_error(PortErrorKind::NotFound, "change.undo_registry.not_found"))
    }

    fn clear(&self) -> Result<(), PortError> {
        self.entries
            .lock()
            .map_err(|_| undo_error(PortErrorKind::Io, "change.undo_registry.unavailable"))?
            .clear();
        Ok(())
    }
}

fn undo_error(kind: PortErrorKind, reason_key: &'static str) -> PortError {
    PortError {
        kind,
        reason_key: reason_key.into(),
        os_code: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{
        ItemRef, OsPath, PathBytes, PathEncoding, Timestamp, platform_os_change::ChangeIdentity,
        ports::EntryKind,
    };

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

    #[test]
    fn trashed_batch_is_idempotent_and_rejects_conflicting_duplicates() -> Result<(), PortError> {
        let registry = SessionUndoInfoRegistry::default();
        let job_ref = id(3);
        let info = undo_info(1, b"trash-a")?;
        registry.register_trashed(&job_ref, &[info.clone(), info.clone()])?;
        assert!(registry.contains(&job_ref, 1)?);

        let conflict = undo_info(1, b"trash-b")?;
        assert!(matches!(
            registry.register_trashed(&job_ref, &[info, conflict]),
            Err(PortError {
                kind: PortErrorKind::Corrupt,
                ..
            })
        ));
        Ok(())
    }

    fn undo_info(item_no: u32, trash_ref: &[u8]) -> Result<TrashedUndoInfo, PortError> {
        let version = id(4);
        Ok(TrashedUndoInfo {
            item_no,
            item_ref: ItemRef {
                result_version: version,
                item_no,
            },
            original_path: OsPath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/fixture/item"),
                display: "/fixture/item".into(),
                lossy: false,
            },
            identity: ChangeIdentity {
                volume_id: b"volume".to_vec(),
                file_id: b"file".to_vec(),
                change_token: None,
            },
            kind: EntryKind::File,
            size: None,
            modified_at: None,
            children_fingerprint: None,
            parent_identity: None,
            trash_location: Some(
                du_core_contracts::platform_os_change::TrashLocationRef::from_adapter(
                    String::from_utf8_lossy(trash_ref).into_owned(),
                ),
            ),
            moved_at: Timestamp::from_unix_millis(1).ok_or_else(|| {
                undo_error(PortErrorKind::Corrupt, "change.undo_registry.invalid_time")
            })?,
        })
    }
}
