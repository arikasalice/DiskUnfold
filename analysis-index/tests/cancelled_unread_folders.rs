mod folder_support;

use du_analysis_index::{
    IngestSink, Observation, QueryError, ResultVersion, SampleSessionKey,
    folder_collection::{UnreadFolderCounts, UnreadReason},
};
use du_core_contracts::{
    ItemRef,
    result::{ResultCompleteness, ResultVersionRead},
};
use folder_support::{dirs, err, folder_path, forest, id, item, root};

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
fn counts_cover_the_target_itself_and_every_marked_folder_below() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = two_root_build(&sink, 1)?;
    mark(&mut build, 0, &["a", "b"], CANCELLED)?;
    mark(&mut build, 0, &["a", "b", "c"], CANCELLED)?;
    mark(&mut build, 0, &["e", "f"], CANCELLED)?;
    mark(&mut build, 0, &["a", "d"], DISCONNECTED)?;
    mark(&mut build, 1, &[], CANCELLED)?;
    let version = build
        .seal(id(2), ResultCompleteness::Cancelled)
        .map_err(err)?;
    // The folder itself is counted, nested marks add up, a disconnect is not counted.
    assert_eq!(below(&version, 0, &["a", "b", "c"])?, Some(1));
    assert_eq!(below(&version, 0, &["a", "b"])?, Some(2));
    assert_eq!(below(&version, 0, &["a"])?, Some(2));
    assert_eq!(below(&version, 0, &["a", "d"])?, Some(0));
    assert_eq!(below(&version, 0, &["e"])?, Some(1));
    assert_eq!(below(&version, 0, &["e", "f"])?, Some(1));
    // A root target counts only its own tree, and a marked root counts itself.
    assert_eq!(below(&version, 0, &[])?, Some(3));
    assert_eq!(below(&version, 1, &[])?, Some(1));
    Ok(())
}

#[test]
fn the_whole_scope_equals_the_version_wide_cancelled_count() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = two_root_build(&sink, 1)?;
    mark(&mut build, 0, &["a", "b"], CANCELLED)?;
    mark(&mut build, 0, &["a", "b", "c"], CANCELLED)?;
    mark(&mut build, 0, &["a", "d"], DISCONNECTED)?;
    mark(&mut build, 1, &[], CANCELLED)?;
    let version = build
        .seal(id(2), ResultCompleteness::Cancelled)
        .map_err(err)?;
    let counts = version.unread_folder_counts();
    assert_eq!(
        counts,
        UnreadFolderCounts {
            cancelled: 3,
            disconnected: 1
        }
    );
    assert_eq!(
        version.cancelled_unread_folders_in_scope(),
        Some(counts.cancelled)
    );
    // The roots partition the scope, so their counts add up to the whole.
    let roots = below(&version, 0, &[])?.unwrap_or(0) + below(&version, 1, &[])?.unwrap_or(0);
    assert_eq!(roots, counts.cancelled);
    Ok(())
}

#[test]
fn a_sealed_version_without_marks_confirms_zero() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = two_root_build(&sink, 1)?;
    mark(&mut build, 0, &["a", "d"], DISCONNECTED)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(version.cancelled_unread_folders_in_scope(), Some(0));
    assert_eq!(below(&version, 0, &[])?, Some(0));
    assert_eq!(below(&version, 0, &["a", "d"])?, Some(0));
    assert_eq!(below(&version, 1, &[])?, Some(0));
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
fn one_cancelled_mark_makes_every_target_count_trustworthy() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = two_root_build(&sink, 1)?;
    mark(&mut build, 0, &["a", "b"], CANCELLED)?;
    let version = build
        .seal(id(2), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(version.cancelled_unread_folders_in_scope(), Some(1));
    assert_eq!(below(&version, 0, &["a"])?, Some(1));
    assert_eq!(below(&version, 0, &["e"])?, Some(0));
    assert_eq!(below(&version, 1, &[])?, Some(0));
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
    assert_eq!(below(&sealed, 0, &["a", "b"])?, Some(1));
    assert_eq!(sealed.cancelled_unread_folders_in_scope(), Some(1));
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
fn a_later_record_or_publication_replaces_the_counts() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("cancelled-below", id(40))
        .map_err(err)?;
    let mut base = sink
        .open_forest_in_lineage(id(41), vec![root(b"/r")], lineage)
        .map_err(err)?;
    base.add_directory_at(0, Vec::new()).map_err(err)?;
    dirs(&mut base, 0, &[&["a"], &["a", "b"], &["c"]])?;
    mark(&mut base, 0, &["a", "b"], CANCELLED)?;
    mark(&mut base, 0, &["c"], CANCELLED)?;
    // The later record for the same folder wins: it is a disconnect now.
    mark(&mut base, 0, &["c"], DISCONNECTED)?;
    let old = base
        .seal(id(42), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(below(&old, 0, &[])?, Some(1));
    assert_eq!(below(&old, 0, &["c"])?, Some(0));

    let mut candidate = sink.open_replacement(&old).map_err(err)?;
    candidate.add_directory_at(0, Vec::new()).map_err(err)?;
    dirs(&mut candidate, 0, &[&["a"], &["a", "b"], &["c"]])?;
    mark(&mut candidate, 0, &["c"], CANCELLED)?;
    let new = candidate
        .seal(id(43), ResultCompleteness::Cancelled)
        .map_err(err)?;
    // Nothing is inherited from the base, and the base keeps its own counts.
    assert_eq!(below(&new, 0, &["a", "b"])?, Some(0));
    assert_eq!(below(&new, 0, &["c"])?, Some(1));
    assert_eq!(below(&new, 0, &[])?, Some(1));
    assert_eq!(below(&old, 0, &["a", "b"])?, Some(1));
    assert_eq!(below(&old, 0, &[])?, Some(1));
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
