//! Whether a folder page shows a lower bound that no row reason explains (#226).
//!
//! A row explains its own lower bound when it is marked as not read, is a boundary, has
//! unread ranges inside, or, for the allocated size, carries a measurement limit. What is
//! left comes from missing folder attempt evidence; the version names that cause once.
use du_analysis_index::{
    ResultVersion,
    folder_collection::{FolderCollection, Unenumerated},
    query::page::Total,
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeValue, browse_ipc::BrowseValue, result::ResultItemKind,
    result::ResultVersionRead,
};

use super::{Error, Row, folder_error};

/// Measurement limits that already explain a lower allocated size.
const EXPLAINING_LIMITS: [&str; 4] = [
    "space.allocated_not_collected",
    "space.placeholder_local_unknown",
    "space.aggregation_overflow",
    "space.dedup_capacity",
];

/// The cause key when the page shows an unexplained lower bound, else `None`. Nothing is
/// judged while the scan runs or for a sample, where the version gives no cause.
///
/// # Errors
/// The opened folder was admitted from this version; a failure maps as its own admission.
pub(super) fn unexplained(
    version: &ResultVersion,
    rows: &[Row],
    total: Total,
    folder: Option<u32>,
) -> Result<Option<&'static str>, Error> {
    let Some(cause) = version.lower_bound_cause() else {
        return Ok(None);
    };
    let count = matches!(total, Total::LowerBound(_)) && !folder_unread(version, folder)?;
    Ok((count || rows.iter().any(|row| row_unexplained(version, row))).then_some(cause))
}

/// A folder row whose size is a lower bound with no reason of its own.
fn row_unexplained(version: &ResultVersion, row: &Row) -> bool {
    if row.observation.kind != ResultItemKind::Directory {
        return false;
    }
    let flags = &row.flags;
    if matches!(flags.unenumerated, BrowseValue::Present { value: true })
        || matches!(flags.boundary, Some(BrowseValue::Present { .. }))
        || matches!(flags.unread_inside, Some(BrowseValue::Present { .. }))
    {
        return false;
    }
    let lower = |size: &Option<SizeValue>| {
        size.as_ref()
            .is_some_and(|size| matches!(size.amount, SizeAmount::LowerBound { .. }))
    };
    let limited = version
        .space(row.observation.item_no)
        .is_some_and(|aggregate| {
            aggregate
                .limitations
                .iter()
                .any(|key| EXPLAINING_LIMITS.contains(key))
        });
    lower(&row.logical_size) || (lower(&row.allocated_size) && !limited)
}

/// Whether the opened folder itself was not read, which already explains its count.
fn folder_unread(version: &ResultVersion, folder: Option<u32>) -> Result<bool, Error> {
    let Some(item_no) = folder else {
        return Ok(false);
    };
    let reference = ItemRef {
        result_version: version.version_id().clone(),
        item_no,
    };
    Ok(matches!(
        version
            .folder_collection(&reference)
            .map_err(folder_error)?,
        FolderCollection::Folder {
            unenumerated: Unenumerated::Yes { .. },
            ..
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_analysis_index::{
        BuildHandle, FileIdentity, IngestSink, Observation, SharingEvidence, SpaceObservation,
    };
    use du_core_contracts::{
        Id, PathBytes, PathEncoding, SizeBasis, SizeEvidence,
        result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
        scan_observation::PlaceholderObservation,
    };
    use std::{num::NonZeroU64, sync::Arc};

    type Test = Result<(), String>;

    fn e<T: std::fmt::Debug>(error: T) -> String {
        format!("{error:?}")
    }

    fn id(n: u8) -> Id {
        Id::from_v7_bytes([n; 16])
    }

    fn open(sink: &IngestSink) -> Result<BuildHandle, String> {
        let root = ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/scope"),
            },
            volume_id: vec![1],
            file_id: vec![1],
        };
        let mut build = sink.open_forest(id(1), vec![root]).map_err(e)?;
        build.add_directory_at(0, Vec::new()).map_err(e)?;
        Ok(build)
    }

    /// Adds `folder/name` with a logical size and, when given, an OS-reported allocation.
    fn file(
        build: &mut BuildHandle,
        folder: &str,
        name: &str,
        file: u8,
        allocated: Option<u64>,
    ) -> Test {
        let size = |basis, bytes: u64| SizeValue {
            basis,
            evidence: SizeEvidence::OsReported,
            amount: SizeAmount::Exact {
                bytes: bytes.into(),
            },
        };
        let path = vec![folder.as_bytes().to_vec(), name.as_bytes().to_vec()];
        build
            .add_with_space_at(
                0,
                Observation {
                    path: path.clone(),
                    size: Some(size(SizeBasis::Logical, 10)),
                },
                SpaceObservation {
                    allocated: allocated.map(|bytes| size(SizeBasis::Allocated, bytes)),
                    identity: Some(FileIdentity {
                        volume: vec![1],
                        file: vec![file],
                    }),
                    link_count: Some(1),
                    sharing: SharingEvidence::None,
                },
            )
            .map_err(e)?;
        let inode = NonZeroU64::new(u64::from(file)).ok_or("zero inode")?;
        build.retain_enumerated_inode(0, &path, inode).map_err(e)
    }

    /// The rows of the scope root's folders, as the page projection builds them.
    fn rows(version: &Arc<ResultVersion>) -> Result<Vec<Row>, String> {
        let items: Vec<ItemRef> = version
            .items()
            .iter()
            .filter(|item| item.parent == Some(0) && item.kind == ResultItemKind::Directory)
            .map(|item| ItemRef {
                result_version: version.version_id().clone(),
                item_no: item.item_no,
            })
            .collect();
        let flags = super::super::collection::page_flags(version, &items).map_err(e)?;
        items
            .into_iter()
            .zip(flags)
            .map(|(reference, flags)| {
                let item = version.detail(&reference).map_err(e)?.item.clone();
                Ok(Row {
                    logical_size: version.item_size(item.item_no, SizeBasis::Logical).cloned(),
                    allocated_size: version
                        .item_size(item.item_no, SizeBasis::Allocated)
                        .cloned(),
                    item: reference,
                    observation: item,
                    display_name: None,
                    modified_at: None,
                    has_observed_child_folder: None,
                    reported_gaps: Vec::new(),
                    flags,
                })
            })
            .collect()
    }

    #[test]
    fn a_placeholder_explains_a_lower_allocated_size() -> Test {
        let sink = IngestSink::default();
        let mut build = open(&sink)?;
        build
            .add_directory_at(0, vec![b"cloud".to_vec()])
            .map_err(e)?;
        file(&mut build, "cloud", "local", 2, Some(4096))?;
        file(&mut build, "cloud", "dataless", 3, None)?;
        build
            .retain_placeholder_observation(
                0,
                &[b"cloud".to_vec(), b"dataless".to_vec()],
                PlaceholderObservation::MacDataless,
            )
            .map_err(e)?;
        let version = build.seal(id(2), ResultCompleteness::Complete).map_err(e)?;
        let rows = rows(&version)?;
        assert!(rows.iter().any(|row| matches!(
            row.allocated_size.as_ref().map(|size| &size.amount),
            Some(SizeAmount::LowerBound { .. })
        )));
        assert_eq!(
            unexplained(&version, &rows, Total::Exact(1), Some(0)),
            Ok(None)
        );
        Ok(())
    }

    #[test]
    fn a_lower_bound_without_a_row_reason_names_the_versions_cause() -> Test {
        let sink = IngestSink::default();
        let mut build = open(&sink)?;
        for folder in ["plain", "gapped"] {
            build
                .add_directory_at(0, vec![folder.as_bytes().to_vec()])
                .map_err(e)?;
            file(
                &mut build,
                folder,
                "f",
                if folder == "plain" { 4 } else { 5 },
                Some(4096),
            )?;
        }
        build
            .record_gap_at(
                0,
                vec![b"gapped".to_vec(), b"x".to_vec()],
                "scan.path_denied".into(),
                None,
            )
            .map_err(e)?;
        let interim = build.publish_partial(id(2)).map_err(e)?;
        assert_eq!(
            unexplained(&interim, &rows(&interim)?, Total::LowerBound(2), Some(0)),
            Ok(None),
            "nothing is judged while the scan runs"
        );
        let version = build.seal(id(3), ResultCompleteness::Missing).map_err(e)?;
        let rows = rows(&version)?;
        // The folder with a gap inside explains itself; the plain one does not.
        let gapped: Vec<Row> = rows
            .iter()
            .filter(|row| matches!(row.flags.unread_inside, Some(BrowseValue::Present { .. })))
            .cloned()
            .collect();
        assert_eq!(gapped.len(), 1);
        assert_eq!(
            unexplained(&version, &gapped, Total::Exact(2), Some(0)),
            Ok(None)
        );
        assert_eq!(
            unexplained(&version, &rows, Total::Exact(2), Some(0)),
            Ok(Some(
                du_analysis_index::folder_collection::REASON_EVIDENCE_NOT_RECORDED
            ))
        );
        // A lower item count of a folder that was read is unexplained as well.
        assert_eq!(
            unexplained(&version, &gapped, Total::LowerBound(2), Some(0)),
            Ok(Some(
                du_analysis_index::folder_collection::REASON_EVIDENCE_NOT_RECORDED
            ))
        );
        Ok(())
    }
}
