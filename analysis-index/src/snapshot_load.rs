//! Isolated construction of a read-only result from a verified snapshot.

use crate::{BuildError, BuildHandle, IngestSink, ResultVersion};
use du_core_contracts::{
    Id, Timestamp,
    data_source::DataSource,
    result::{ResultCompleteness, ResultScopeRoot},
};
use std::sync::Arc;

/// A private build for one already verified snapshot.
///
/// This owns a result store that is not shared with a live analysis. Dropping an
/// unfinished build aborts it; sealing returns the immutable result and leaves
/// its lineage unset. The required snapshot ID is bound before any item is added.
pub struct SnapshotBuild {
    build: BuildHandle,
    version_id: Id,
    observed_at: Option<Timestamp>,
    completeness: ResultCompleteness,
}

impl SnapshotBuild {
    /// Opens an isolated snapshot build with its immutable result metadata.
    ///
    /// `snapshot_id` is required, so this API cannot create a snapshot-sourced
    /// result without its identity. Roots must be non-empty and any fingerprint
    /// must be the canonical lowercase hexadecimal form retained by DUSNAP v1.
    pub fn open(
        snapshot_id: Id,
        version_id: Id,
        scope_version_id: Id,
        roots: Vec<ResultScopeRoot>,
        scope_fingerprint: Option<String>,
        observed_at: Option<Timestamp>,
        completeness: ResultCompleteness,
    ) -> Result<Self, BuildError> {
        if completeness == ResultCompleteness::Cancelled
            || scope_fingerprint
                .as_deref()
                .is_some_and(|value| !canonical_fingerprint(value))
        {
            return Err(BuildError::InvalidTree);
        }
        let sink = IngestSink::default();
        let mut build = sink.open_forest(scope_version_id, roots)?;
        build.data_source = DataSource::Snapshot;
        build.snapshot_id = Some(snapshot_id);
        build.scope_fingerprint = scope_fingerprint;
        Ok(Self {
            build,
            version_id,
            observed_at,
            completeness,
        })
    }

    /// Returns the isolated build so the caller can use the existing ingest calls.
    pub fn build_mut(&mut self) -> &mut BuildHandle {
        &mut self.build
    }

    /// Seals and returns the immutable snapshot result.
    pub fn seal(self) -> Result<Arc<ResultVersion>, BuildError> {
        self.build
            .seal_at(self.version_id, self.completeness, self.observed_at)
    }
}

impl ResultVersion {
    /// Returns the snapshot identity when this result was loaded from a snapshot.
    pub fn snapshot_id(&self) -> Option<&Id> {
        self.snapshot_id.as_ref()
    }
}

fn canonical_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder_collection;
    use du_core_contracts::{
        ItemRef, PathBytes, PathEncoding, data_source::DataSource, result::ResultVersionRead,
    };

    fn id(number: u32) -> Result<Id, String> {
        Id::parse(&format!("018f0000-0000-7000-8000-{number:012x}"))
    }

    fn root() -> ResultScopeRoot {
        ResultScopeRoot {
            path: du_core_contracts::result::ResultNativePath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/snapshot"),
            },
            volume_id: b"volume".to_vec(),
            file_id: b"root".to_vec(),
        }
    }

    #[test]
    fn snapshot_build_binds_identity_without_touching_a_live_store() -> Result<(), String> {
        let live = IngestSink::default();
        let mut current = live
            .open_forest(id(1)?, vec![root()])
            .map_err(|e| format!("{e:?}"))?;
        current
            .add_directory_at(0, Vec::new())
            .map_err(|e| format!("{e:?}"))?;
        let current = current
            .seal(id(2)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;

        let mut snapshot = SnapshotBuild::open(
            id(3)?,
            id(4)?,
            id(5)?,
            vec![root()],
            Some("a".repeat(64)),
            Some(Timestamp::from_unix_millis(1_700_000_000_000).ok_or("time")?),
            ResultCompleteness::Complete,
        )
        .map_err(|e| format!("{e:?}"))?;
        snapshot
            .build_mut()
            .add_directory_at(0, Vec::new())
            .map_err(|e| format!("{e:?}"))?;
        let loaded = snapshot.seal().map_err(|e| format!("{e:?}"))?;

        assert_eq!(loaded.data_source(), DataSource::Snapshot);
        assert_eq!(loaded.snapshot_id(), Some(&id(3)?));
        assert_eq!(loaded.lineage_id(), None);
        assert!(Arc::ptr_eq(
            &current,
            &live.pin().map_err(|e| format!("{e:?}"))?.ok_or("current")?
        ));
        Ok(())
    }

    #[test]
    fn snapshot_folder_facts_require_a_recorded_marker() -> Result<(), String> {
        let mut snapshot = SnapshotBuild::open(
            id(10)?,
            id(11)?,
            id(12)?,
            vec![root()],
            None,
            None,
            ResultCompleteness::Partial,
        )
        .map_err(|e| format!("{e:?}"))?;
        let build = snapshot.build_mut();
        build
            .add_directory_at(0, Vec::new())
            .map_err(|e| format!("{e:?}"))?;
        build
            .add_directory_at(0, vec![b"denied".to_vec()])
            .map_err(|e| format!("{e:?}"))?;
        build
            .record_gap_at(0, vec![b"denied".to_vec()], "scan.path_denied".into(), None)
            .map_err(|e| format!("{e:?}"))?;
        let version = snapshot.seal().map_err(|e| format!("{e:?}"))?;
        let reference = |item_no| ItemRef {
            result_version: version.version_id().clone(),
            item_no,
        };
        assert_eq!(
            version
                .folder_collection(&reference(0))
                .map_err(|e| format!("{e:?}"))?,
            folder_collection::FolderCollection::Folder {
                unenumerated: folder_collection::Unenumerated::Unknown {
                    reason_key: folder_collection::REASON_DATA_INSUFFICIENT,
                },
                boundary: folder_collection::FolderBoundary::NotProvided,
            }
        );
        assert_eq!(
            version
                .folder_collection(&reference(1))
                .map_err(|e| format!("{e:?}"))?,
            folder_collection::FolderCollection::Folder {
                unenumerated: folder_collection::Unenumerated::Yes {
                    reason_key: "scan.path_denied",
                },
                boundary: folder_collection::FolderBoundary::Unknown {
                    reason_key: folder_collection::REASON_DATA_INSUFFICIENT,
                },
            }
        );
        Ok(())
    }

    #[test]
    fn ordinary_builds_have_no_snapshot_identity() -> Result<(), String> {
        let sink = IngestSink::default();
        let mut build = sink
            .open_forest(id(20)?, vec![root()])
            .map_err(|e| format!("{e:?}"))?;
        build
            .add_directory_at(0, Vec::new())
            .map_err(|e| format!("{e:?}"))?;
        let version = build
            .seal(id(21)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(version.data_source(), DataSource::Real);
        assert_eq!(version.snapshot_id(), None);
        Ok(())
    }
}
