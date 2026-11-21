mod folder_support;

use du_analysis_index::{
    FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence, SpaceObservation,
    folder_collection::{ExcludedChildren, UnreadFolderCounts, UnreadReason},
};
use du_core_contracts::{
    ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead},
};
use folder_support::{
    UNPROVEN, dirs, err, folder_path, forest, gap, id, item, state, unenumerated,
};

const CANCELLED: UnreadReason = UnreadReason::Cancelled;
const DISCONNECTED: UnreadReason = UnreadReason::Disconnected;

fn excluded(version: &ResultVersion, parts: &[&str]) -> Result<Option<ExcludedChildren>, String> {
    version
        .folder_excluded_children(&item(version, 0, parts)?)
        .map_err(err)
}

fn counts(cancelled: u32, disconnected: u32) -> UnreadFolderCounts {
    UnreadFolderCounts {
        cancelled,
        disconnected,
    }
}

#[test]
fn counts_are_per_reason_and_separate_from_ranges_and_issues() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 2)?;
    dirs(&mut build, 0, &[&["a"], &["b"], &["c"], &["d"]])?;
    for name in ["a", "b", "c"] {
        build
            .set_directory_unread(0, &folder_path(&[name]), CANCELLED)
            .map_err(err)?;
    }
    build
        .set_directory_unread(0, &folder_path(&["d"]), DISCONNECTED)
        .map_err(err)?;
    build
        .set_directory_unread(1, &[], DISCONNECTED)
        .map_err(err)?;
    // A mark on a folder that also has a gap is still counted as a mark.
    gap(&mut build, 0, &["a"], "scan.path_denied")?;
    let version = build
        .seal(id(2), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(version.unread_folder_counts(), counts(3, 2));
    // Only the one real gap counts as an inaccessible range; marks add nothing.
    assert_eq!(version.scope_gaps().len(), 1);
    assert!(version.scan_issues().is_empty());
    assert_eq!(version.scope_total().inaccessible_ranges, Some(1));
    let none = IngestSink::default();
    let mut build = forest(&none, 1, 1)?;
    dirs(&mut build, 0, &[&["a"]])?;
    let version = build
        .seal(id(2), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(
        version.unread_folder_counts(),
        UnreadFolderCounts::default()
    );
    Ok(())
}

#[test]
fn replacement_builds_count_only_their_own_marks() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("folder-unread", id(40))
        .map_err(err)?;
    let mut base = sink
        .open_forest_in_lineage(id(41), vec![folder_support::root(b"/r")], lineage)
        .map_err(err)?;
    base.add_directory_at(0, Vec::new()).map_err(err)?;
    dirs(&mut base, 0, &[&["a"]])?;
    base.set_directory_unread(0, &folder_path(&["a"]), CANCELLED)
        .map_err(err)?;
    let old = base
        .seal(id(42), ResultCompleteness::Cancelled)
        .map_err(err)?;
    let mut candidate = sink.open_replacement(&old).map_err(err)?;
    candidate.add_directory_at(0, Vec::new()).map_err(err)?;
    dirs(&mut candidate, 0, &[&["a"]])?;
    let new = candidate
        .seal(id(43), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(state(&new, 0, &["a"])?, UNPROVEN);
    assert_eq!(new.unread_folder_counts(), UnreadFolderCounts::default());
    assert_eq!(state(&old, 0, &["a"])?, unenumerated("scan.cancelled"));
    assert_eq!(old.unread_folder_counts(), counts(1, 0));
    Ok(())
}

#[test]
fn a_marked_folder_never_reports_an_exact_excluded_count() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["marked"], &["plain"], &["bare"]])?;
    for name in ["marked", "plain"] {
        build
            .set_excluded_children(0, &folder_path(&[name]), 4, true)
            .map_err(err)?;
    }
    build
        .set_directory_unread(0, &folder_path(&["marked"]), CANCELLED)
        .map_err(err)?;
    build
        .set_directory_unread(0, &folder_path(&["bare"]), CANCELLED)
        .map_err(err)?;
    let version = build
        .seal(id(2), ResultCompleteness::Missing)
        .map_err(err)?;

    assert_eq!(excluded(&version, &["marked"])?, None);
    assert_eq!(excluded(&version, &["plain"])?, None);
    assert_eq!(excluded(&version, &["bare"])?, None);
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["marked"], &["bare"], &["plain"]])?;
    build
        .set_excluded_children(0, &folder_path(&["marked"]), 2, true)
        .map_err(err)?;
    build
        .set_directory_unread(0, &folder_path(&["marked"]), CANCELLED)
        .map_err(err)?;
    build
        .set_directory_unread(0, &folder_path(&["bare"]), CANCELLED)
        .map_err(err)?;
    let cancelled = build
        .seal(id(3), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(excluded(&cancelled, &["marked"])?, None);
    // An unread mark does not establish a count.
    assert_eq!(excluded(&cancelled, &["bare"])?, None);
    // A folder without a mark, a gap or a record stays unknown in a cancelled version.
    assert_eq!(excluded(&cancelled, &["plain"])?, None);
    Ok(())
}

fn size(bytes: u64, basis: SizeBasis, evidence: SizeEvidence) -> SizeValue {
    SizeValue {
        basis,
        evidence,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(bytes),
        },
    }
}

/// Two roots with files; the folders `marked` (empty) and `nest/marked` are unread.
fn cancelled_scenario(
    number: u8,
    completeness: ResultCompleteness,
    marks: bool,
) -> Result<std::sync::Arc<ResultVersion>, String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 2)?;
    dirs(
        &mut build,
        0,
        &[
            &["marked"],
            &["nest"],
            &["nest", "marked"],
            &["nest", "read"],
        ],
    )?;
    for (root, parts) in [
        (0, vec!["nest", "read", "f"]),
        (0, vec!["g"]),
        (1, vec!["h"]),
    ] {
        let physical = SpaceObservation {
            allocated: Some(size(16, SizeBasis::Allocated, SizeEvidence::OsReported)),
            identity: Some(FileIdentity {
                volume: vec![1],
                file: vec![root as u8, parts.len() as u8],
            }),
            link_count: Some(1),
            sharing: SharingEvidence::None,
        };
        let observation = Observation {
            path: folder_path(&parts),
            size: Some(size(10, SizeBasis::Logical, SizeEvidence::Measured)),
        };
        build
            .add_with_space_at(root, observation, physical)
            .map_err(err)?;
    }
    if marks {
        build
            .set_directory_unread(0, &folder_path(&["marked"]), CANCELLED)
            .map_err(err)?;
        build
            .set_directory_unread(0, &folder_path(&["nest", "marked"]), CANCELLED)
            .map_err(err)?;
    }
    build.seal(id(number), completeness).map_err(err)
}

fn is_exact(value: &SizeValue) -> bool {
    matches!(value.amount, SizeAmount::Exact { .. })
}

#[test]
fn cancelled_and_partial_versions_make_marked_folders_and_ancestors_inexact() -> Result<(), String>
{
    for (number, completeness) in [
        (2, ResultCompleteness::Cancelled),
        (3, ResultCompleteness::Partial),
    ] {
        let marked = cancelled_scenario(number, completeness, true)?;
        let plain = cancelled_scenario(number, completeness, false)?;
        for target in [&["marked"][..], &["nest", "marked"], &["nest"], &[]] {
            let reference = item(&marked, 0, target)?;
            let aggregate = marked.space(reference.item_no).ok_or("no aggregate")?;
            for value in [&aggregate.logical, &aggregate.allocated] {
                assert!(!is_exact(value), "{target:?} is exact in {completeness:?}");
            }
            assert_eq!(aggregate.completeness, completeness);
            assert_eq!(aggregate.inaccessible_ranges, None);
        }
        // The empty marked folder has nothing observed, so its size is unknown, not zero.
        let leaf = item(&marked, 0, &["marked"])?;
        let leaf = marked.space(leaf.item_no).ok_or("no aggregate")?;
        assert_eq!(leaf.logical.amount, SizeAmount::Unknown);
        // The mark does not touch aggregation at all.
        let all = |version: &ResultVersion| -> Vec<_> {
            (0..version.items().len() as u32)
                .filter_map(|n| version.space(n))
                .chain([version.scope_total().clone()])
                .collect()
        };
        assert_eq!(all(&marked), all(&plain));
        assert!(!is_exact(&marked.scope_total().logical));
        assert!(marked.items().iter().all(|i| {
            i.kind != ResultItemKind::Directory
                || marked
                    .space(i.item_no)
                    .is_some_and(|a| !is_exact(&a.logical))
        }));
    }
    Ok(())
}
