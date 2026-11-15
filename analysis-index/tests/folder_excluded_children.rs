mod folder_support;

use du_analysis_index::{
    BuildError, IngestSink, Observation, QueryError, ResultVersion, SampleSessionKey,
    folder_collection::ExcludedChildren,
};
use du_core_contracts::{ItemRef, result::ResultCompleteness};
use folder_support::{dirs, err, folder_path, forest, gap, id, item, root};

fn excluded(
    version: &ResultVersion,
    root_index: usize,
    parts: &[&str],
) -> Result<Option<ExcludedChildren>, String> {
    version
        .folder_excluded_children(&item(version, root_index, parts)?)
        .map_err(err)
}

fn some(count: u32, exact: bool) -> Option<ExcludedChildren> {
    Some(ExcludedChildren { count, exact })
}

#[test]
fn record_validation_matches_the_directory_case_contract() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 2)?;
    dirs(&mut build, 0, &[&["dir"]])?;
    build
        .add_file_at(
            0,
            Observation {
                path: folder_path(&["file"]),
                size: None,
            },
        )
        .map_err(err)?;
    assert_eq!(
        build.set_excluded_children(0, &folder_path(&["dir"]), 1, true),
        Ok(())
    );
    assert_eq!(build.set_excluded_children(1, &[], 1, true), Ok(()));
    let invalid = Err(BuildError::InvalidTree);
    assert_eq!(
        build.set_excluded_children(0, &folder_path(&["file"]), 1, true),
        invalid
    );
    assert_eq!(
        build.set_excluded_children(0, &folder_path(&["none"]), 1, true),
        invalid
    );
    assert_eq!(build.set_excluded_children(2, &[], 1, true), invalid);
    assert_eq!(
        build.set_excluded_children(0, &[Vec::new()], 1, true),
        invalid
    );
    assert_eq!(
        build.set_excluded_children(1, &folder_path(&["dir"]), 1, true),
        invalid
    );
    // A rejected record leaves the build usable.
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(excluded(&version, 0, &["dir"])?, some(1, true));
    Ok(())
}

#[test]
fn a_later_record_replaces_the_earlier_one_in_future_publications() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["dir"]])?;
    build
        .set_excluded_children(0, &folder_path(&["dir"]), 3, false)
        .map_err(err)?;
    let interim = build.publish_partial(id(2)).map_err(err)?;
    build
        .set_excluded_children(0, &folder_path(&["dir"]), 5, true)
        .map_err(err)?;
    let sealed = build
        .seal(id(3), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(excluded(&interim, 0, &["dir"])?, some(3, false));
    assert_eq!(excluded(&sealed, 0, &["dir"])?, some(5, true));
    Ok(())
}

#[test]
fn recorded_count_is_exact_only_for_a_complete_folder_without_a_gap() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 2)?;
    dirs(
        &mut build,
        0,
        &[&["whole"], &["cut"], &["denied"], &["empty"]],
    )?;
    for (name, count, complete) in [
        ("whole", 4, true),
        ("cut", 2, false),
        ("denied", 7, true),
        ("empty", 0, true),
    ] {
        build
            .set_excluded_children(0, &folder_path(&[name]), count, complete)
            .map_err(err)?;
    }
    build.set_excluded_children(1, &[], 9, true).map_err(err)?;
    gap(&mut build, 0, &["denied"], "scan.path_denied")?;
    let version = build
        .seal(id(2), ResultCompleteness::Partial)
        .map_err(err)?;
    assert_eq!(excluded(&version, 0, &["whole"])?, some(4, true));
    assert_eq!(excluded(&version, 0, &["cut"])?, some(2, false));
    assert_eq!(excluded(&version, 0, &["denied"])?, some(7, false));
    assert_eq!(excluded(&version, 0, &["empty"])?, some(0, true));
    assert_eq!(excluded(&version, 1, &[])?, some(9, true));
    Ok(())
}

#[test]
fn unrecorded_folders_are_zero_in_a_sealed_complete_or_missing_version() -> Result<(), String> {
    for (number, completeness) in [
        (2, ResultCompleteness::Complete),
        (3, ResultCompleteness::Missing),
    ] {
        let sink = IngestSink::default();
        let mut build = forest(&sink, 1, 1)?;
        dirs(&mut build, 0, &[&["a"], &["denied"]])?;
        gap(&mut build, 0, &["denied"], "scan.path_denied")?;
        let version = build.seal(id(number), completeness).map_err(err)?;
        assert_eq!(excluded(&version, 0, &["a"])?, some(0, true));
        assert_eq!(excluded(&version, 0, &[])?, some(0, true));
        // Unread entries cannot be shown to hold no excluded child.
        assert_eq!(excluded(&version, 0, &["denied"])?, some(0, false));
    }
    Ok(())
}

#[test]
fn unrecorded_folders_are_unknown_in_progress_partial_cancelled_and_sample() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"]])?;
    let interim = build.publish_partial(id(2)).map_err(err)?;
    assert_eq!(excluded(&interim, 0, &["a"])?, None);
    let sealed = build
        .seal(id(3), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(excluded(&sealed, 0, &["a"])?, None);
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"]])?;
    let partial = build
        .seal(id(4), ResultCompleteness::Partial)
        .map_err(err)?;
    assert_eq!(excluded(&partial, 0, &["a"])?, None);
    let sink = IngestSink::default();
    let mut sample = sink
        .open_sample_forest(id(1), vec![root(b"/s")], &SampleSessionKey::new(id(9)))
        .map_err(err)?;
    sample.add_directory_at(0, Vec::new()).map_err(err)?;
    sample.set_excluded_children(0, &[], 7, true).map_err(err)?;
    let version = sample
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(excluded(&version, 0, &[])?, None);
    Ok(())
}

#[test]
fn partial_seal_reports_only_the_recorded_folders() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"], &["b"]])?;
    build
        .set_excluded_children(0, &folder_path(&["a"]), 1, true)
        .map_err(err)?;
    let version = build
        .seal(id(2), ResultCompleteness::Partial)
        .map_err(err)?;
    assert_eq!(excluded(&version, 0, &["a"])?, some(1, true));
    assert_eq!(excluded(&version, 0, &["b"])?, None);
    Ok(())
}

#[test]
fn non_directories_and_foreign_references_are_errors() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    build
        .add_file_at(
            0,
            Observation {
                path: folder_path(&["f"]),
                size: None,
            },
        )
        .map_err(err)?;
    build
        .add_at(
            0,
            Observation {
                path: folder_path(&["odd"]),
                size: None,
            },
        )
        .map_err(err)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let file = item(&version, 0, &["f"])?;
    let odd = item(&version, 0, &["odd"])?;
    assert_eq!(
        version.folder_excluded_children(&file),
        Err(QueryError::NotDirectory)
    );
    assert_eq!(
        version.folder_excluded_children(&odd),
        Err(QueryError::KindUnknown)
    );
    let stale = ItemRef {
        result_version: id(7),
        item_no: 0,
    };
    assert_eq!(
        version.folder_excluded_children(&stale),
        Err(QueryError::StaleVersion)
    );
    let missing = ItemRef {
        result_version: file.result_version,
        item_no: u32::MAX,
    };
    assert_eq!(
        version.folder_excluded_children(&missing),
        Err(QueryError::NotFound)
    );
    Ok(())
}
