mod folder_support;

use du_analysis_index::{
    IngestSink, Observation, QueryError, ResultVersion, SampleSessionKey,
    folder_collection::UnreadReason,
};
use du_core_contracts::{
    ItemRef,
    result::{ResultCompleteness, ResultVersionRead},
};
use folder_support::{dirs, err, folder_path, forest, gap, id, item, root};

const CANCELLED: UnreadReason = UnreadReason::Cancelled;
const DISCONNECTED: UnreadReason = UnreadReason::Disconnected;

fn mark(
    build: &mut du_analysis_index::BuildHandle,
    root: u32,
    parts: &[&str],
    reason: UnreadReason,
) -> Result<(), String> {
    build
        .set_directory_unread(root, &folder_path(parts), reason)
        .map_err(err)
}

fn below(version: &ResultVersion, root: usize, parts: &[&str]) -> Result<Option<u32>, String> {
    version
        .cancelled_unread_folders(&item(version, root, parts)?)
        .map_err(err)
}

/// Root 0 holds a, a/b, a/b/c, a/d, e and e/f; root 1 is a bare root.
fn two_root_build(
    sink: &IngestSink,
    version: u8,
) -> Result<du_analysis_index::BuildHandle, String> {
    let mut build = forest(sink, version, 2)?;
    dirs(
        &mut build,
        0,
        &[
            &["a"],
            &["a", "b"],
            &["a", "b", "c"],
            &["a", "d"],
            &["e"],
            &["e", "f"],
        ],
    )?;
    Ok(build)
}

#[test]
fn a_cancelled_mark_does_not_prove_a_partial_or_cancelled_total() -> Result<(), String> {
    for (number, completeness) in [
        (2, ResultCompleteness::Cancelled),
        (3, ResultCompleteness::Partial),
    ] {
        let sink = IngestSink::default();
        let mut build = two_root_build(&sink, 1)?;
        mark(&mut build, 0, &["a", "b"], CANCELLED)?;
        let version = build.seal(id(number), completeness).map_err(err)?;

        assert_eq!(version.unread_folder_counts().cancelled, 1);
        assert_eq!(version.cancelled_unread_folders_in_scope(), None);
        assert_eq!(below(&version, 0, &["a", "b"])?, None);
        assert_eq!(below(&version, 0, &["a"])?, None);
        assert_eq!(below(&version, 0, &["e"])?, None);
        assert_eq!(below(&version, 1, &[])?, None);
    }
    Ok(())
}

#[test]
fn a_sealed_complete_or_missing_scan_without_cancelled_marks_confirms_zero() -> Result<(), String> {
    for (number, expected) in [
        (2, ResultCompleteness::Complete),
        (3, ResultCompleteness::Missing),
    ] {
        let sink = IngestSink::default();
        let mut build = two_root_build(&sink, 1)?;
        if expected == ResultCompleteness::Missing {
            // Completing with a known gap seals as Missing through the production index path.
            gap(&mut build, 0, &["a", "d"], "scan.path_denied")?;
        }
        let version = build
            .seal(id(number), ResultCompleteness::Complete)
            .map_err(err)?;

        assert_eq!(version.completeness(), expected);
        assert_eq!(version.cancelled_unread_folders_in_scope(), Some(0));
        assert_eq!(below(&version, 0, &[])?, Some(0));
        assert_eq!(below(&version, 0, &["a", "d"])?, Some(0));
        assert_eq!(below(&version, 1, &[])?, Some(0));
    }
    Ok(())
}

#[test]
fn a_cancelled_mark_contradicts_a_sealed_complete_or_missing_result() -> Result<(), String> {
    for (number, expected) in [
        (2, ResultCompleteness::Complete),
        (3, ResultCompleteness::Missing),
    ] {
        let sink = IngestSink::default();
        let mut build = two_root_build(&sink, 1)?;
        mark(&mut build, 0, &["a", "b"], CANCELLED)?;
        if expected == ResultCompleteness::Missing {
            gap(&mut build, 0, &["a", "d"], "scan.path_denied")?;
        }
        let version = build
            .seal(id(number), ResultCompleteness::Complete)
            .map_err(err)?;

        assert_eq!(version.completeness(), expected);
        assert_eq!(version.unread_folder_counts().cancelled, 1);
        assert_eq!(version.cancelled_unread_folders_in_scope(), None);
        assert_eq!(below(&version, 0, &[])?, None);
        assert_eq!(below(&version, 0, &["a", "b"])?, None);
    }
    Ok(())
}

#[test]
fn a_real_incremental_replacement_cannot_confirm_zero_cancelled_unread() -> Result<(), String> {
    for (base_version, completeness) in [
        (20, ResultCompleteness::Complete),
        (30, ResultCompleteness::Missing),
    ] {
        let sink = IngestSink::default();
        let lineage = sink
            .bind_current_analysis("cancelled-below-replacement", id(base_version))
            .map_err(err)?;
        let mut base = sink
            .open_forest_in_lineage(id(base_version + 1), vec![root(b"/r")], lineage)
            .map_err(err)?;
        base.add_directory_at(0, Vec::new()).map_err(err)?;
        dirs(&mut base, 0, &[&["a"], &["a", "b"], &["a", "d"]])?;
        mark(&mut base, 0, &["a", "b"], CANCELLED)?;
        let base = base
            .seal(id(base_version + 2), ResultCompleteness::Cancelled)
            .map_err(err)?;

        let mut replacement = sink.open_replacement(&base).map_err(err)?;
        replacement.add_directory_at(0, Vec::new()).map_err(err)?;
        dirs(&mut replacement, 0, &[&["a"], &["a", "b"], &["a", "d"]])?;
        if completeness == ResultCompleteness::Missing {
            gap(&mut replacement, 0, &["a", "d"], "scan.path_denied")?;
        }
        let version = replacement
            .seal(id(base_version + 3), completeness)
            .map_err(err)?;

        assert_eq!(base.origin(), "scan");
        assert_eq!(version.origin(), "incremental");
        assert_eq!(version.completeness(), completeness);
        assert_eq!(version.unread_folder_counts().cancelled, 0);
        assert_eq!(version.cancelled_unread_folders_in_scope(), None);
        assert_eq!(below(&version, 0, &[])?, None);
        assert_eq!(below(&version, 0, &["a", "b"])?, None);
    }
    Ok(())
}

#[test]
fn a_cancelled_or_partial_version_without_cancelled_marks_does_not_confirm_zero()
-> Result<(), String> {
    for (number, completeness) in [
        (2, ResultCompleteness::Cancelled),
        (3, ResultCompleteness::Partial),
    ] {
        let sink = IngestSink::default();
        let mut build = two_root_build(&sink, 1)?;
        // A disconnect is recorded, but the collector reported no cancelled folder.
        mark(&mut build, 0, &["a", "d"], DISCONNECTED)?;
        let version = build.seal(id(number), completeness).map_err(err)?;
        assert_eq!(below(&version, 0, &["a"])?, None, "{completeness:?}");
        assert_eq!(below(&version, 0, &[])?, None, "{completeness:?}");
        assert_eq!(version.cancelled_unread_folders_in_scope(), None);
    }
    Ok(())
}

#[test]
fn an_unfinished_publication_does_not_provide_the_count() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = two_root_build(&sink, 1)?;
    mark(&mut build, 0, &["a", "b"], CANCELLED)?;
    let interim = build.publish_partial(id(2)).map_err(err)?;
    assert_eq!(below(&interim, 0, &["a", "b"])?, None);
    assert_eq!(below(&interim, 0, &[])?, None);
    assert_eq!(interim.cancelled_unread_folders_in_scope(), None);
    let sealed = build
        .seal(id(3), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(below(&sealed, 0, &["a", "b"])?, None);
    assert_eq!(sealed.cancelled_unread_folders_in_scope(), None);
    // The interim version stays pinned with its own answer.
    assert_eq!(interim.cancelled_unread_folders_in_scope(), None);
    Ok(())
}

#[test]
fn a_sample_does_not_provide_the_count_even_with_marks() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_sample_forest(id(1), vec![root(b"/s")], &SampleSessionKey::new(id(9)))
        .map_err(err)?;
    build.add_directory_at(0, Vec::new()).map_err(err)?;
    dirs(&mut build, 0, &[&["a"]])?;
    mark(&mut build, 0, &["a"], CANCELLED)?;
    let version = build
        .seal(id(2), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(below(&version, 0, &["a"])?, None);
    assert_eq!(below(&version, 0, &[])?, None);
    assert_eq!(version.cancelled_unread_folders_in_scope(), None);
    Ok(())
}

#[test]
fn only_folders_of_the_pinned_version_are_answered() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = two_root_build(&sink, 1)?;
    build
        .add_file_at(
            0,
            Observation {
                path: folder_path(&["a", "file"]),
                size: None,
            },
        )
        .map_err(err)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let file = item(&version, 0, &["a", "file"])?;
    assert_eq!(
        version.cancelled_unread_folders(&file),
        Err(QueryError::NotDirectory)
    );
    let stale = ItemRef {
        result_version: id(9),
        item_no: 0,
    };
    assert_eq!(
        version.cancelled_unread_folders(&stale),
        Err(QueryError::StaleVersion)
    );
    let missing = ItemRef {
        result_version: version.version_id().clone(),
        item_no: 9_999,
    };
    assert_eq!(
        version.cancelled_unread_folders(&missing),
        Err(QueryError::NotFound)
    );
    Ok(())
}
