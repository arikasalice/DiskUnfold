//! Snapshot index adapter over the application-owned SQLite connection.

use std::sync::{Arc, Mutex};

use du_core_contracts::{
    Id,
    snapshot_index::{
        SnapshotIndexCursor, SnapshotIndexError, SnapshotIndexPage, SnapshotIndexPort,
        SnapshotIndexRegisterResult, SnapshotIndexRow, SnapshotIndexVolume,
    },
};
use du_engine_api::{api::EngineBuilder, desktop::DesktopServices};
use du_platform_os::snapshot_file_system::NativeSnapshotFileSystem;
use du_snapshot_files::SnapshotFileStore;

use crate::shared_database::SharedDatabase;

pub(super) struct SharedSnapshotIndex(SharedDatabase);

impl SnapshotIndexPort for SharedSnapshotIndex {
    fn register(
        &mut self,
        row: &SnapshotIndexRow,
        volumes: &[SnapshotIndexVolume],
        baseline_pinned_at_ms: Option<i64>,
    ) -> Result<SnapshotIndexRegisterResult, SnapshotIndexError> {
        self.0
            .0
            .connection
            .lock()
            .map_err(|_| SnapshotIndexError::Storage)?
            .register(row, volumes, baseline_pinned_at_ms)
    }

    fn find_active_by_source(
        &mut self,
        source_version_id: &Id,
    ) -> Result<Option<Id>, SnapshotIndexError> {
        self.0
            .0
            .connection
            .lock()
            .map_err(|_| SnapshotIndexError::Storage)?
            .find_active_by_source(source_version_id)
    }

    fn list_active(
        &mut self,
        limit: usize,
        after: Option<&SnapshotIndexCursor>,
    ) -> Result<SnapshotIndexPage, SnapshotIndexError> {
        self.0
            .0
            .connection
            .lock()
            .map_err(|_| SnapshotIndexError::Storage)?
            .list_active(limit, after)
    }
}

/// Registers the F07 command with the window-owned SQLite and OS file adapters.
pub(super) fn register(
    services: &DesktopServices,
    builder: &mut EngineBuilder,
    database: &SharedDatabase,
) -> Result<(), Box<dyn std::error::Error>> {
    let files = Arc::new(SnapshotFileStore::new(NativeSnapshotFileSystem::new()?));
    let index = Arc::new(Mutex::new(SharedSnapshotIndex(database.clone())));
    services.register_snapshot_save(builder, index.clone(), files, true)?;
    services.register_snapshot_list(builder, index, true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_app_db::AppDb;
    use du_core_contracts::snapshot_index::{
        SnapshotIndexCompleteness, SnapshotIndexPort, SnapshotIndexRow, SnapshotIndexSourceKind,
        SnapshotIndexState, SnapshotIndexVolume, SnapshotVolumeIdentityCapability,
        SnapshotVolumeIdentityKind,
    };

    fn id(value: u32) -> Result<Id, Box<dyn std::error::Error>> {
        Ok(Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}"))?)
    }

    #[test]
    fn shared_index_commits_snapshot_volume_baseline_and_sequence()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = AppDb::create_gui_with_history(&directory.path().join("app.db"))
            .map_err(|error| format!("{error:?}"))?;
        let shared = SharedDatabase::new(database, None);
        let mut index = SharedSnapshotIndex(shared);
        let row = SnapshotIndexRow {
            snapshot_id: id(1)?,
            file_name: "018f0000-0000-7000-8000-000000000001.dusnap".into(),
            source_version_id: id(2)?,
            lineage_id: id(3)?,
            source_kind: SnapshotIndexSourceKind::Local,
            observed_at_ms: Some(100),
            created_at_ms: 200,
            scope_label: Vec::new(),
            condition_summary: "{\"state\":\"unknown\"}".into(),
            completeness: Some(SnapshotIndexCompleteness::Complete),
            format_version: 1,
            body_size: 256,
            body_hash: [7; 32],
            state: SnapshotIndexState::Active,
            verify_failed_at_ms: None,
            updated_at_ms: 200,
            row_version: 1,
        };
        let volumes = [SnapshotIndexVolume {
            volume_index: 0,
            observed_volume_id: Some(vec![4, 5]),
            identity_kind: SnapshotVolumeIdentityKind::None,
            identity_capability: SnapshotVolumeIdentityCapability::Unknown,
        }];

        let registered = index.register(&row, &volumes, Some(200))?;
        assert!(registered.inserted);
        assert_eq!(registered.list_seq, 1);
        let page = index.list_active(10, None)?;
        assert_eq!(page.list_seq, registered.list_seq);
        assert_eq!(page.rows.len(), 1);
        let entry = &page.rows[0];
        assert_eq!(entry.row, row);
        assert_eq!(entry.volumes, volumes);
        assert!(entry.pinned);
        assert_eq!(
            index.find_active_by_source(&row.source_version_id)?,
            Some(row.snapshot_id)
        );
        Ok(())
    }
}
