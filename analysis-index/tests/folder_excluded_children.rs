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
    assert_eq!(excluded(&version, 0, &["dir"])?, None);
    Ok(())
}

#[test]
fn reported_counts_are_omitted_in_interim_and_final_versions() -> Result<(), String> {
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
    assert_eq!(excluded(&interim, 0, &["dir"])?, None);
    assert_eq!(excluded(&sealed, 0, &["dir"])?, None);
    Ok(())
}

#[test]
fn reported_counts_are_omitted_without_a_distinctness_guarantee() -> Result<(), String> {
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
    assert_eq!(excluded(&version, 0, &["whole"])?, None);
    assert_eq!(excluded(&version, 0, &["cut"])?, None);
    assert_eq!(excluded(&version, 0, &["denied"])?, None);
    assert_eq!(excluded(&version, 0, &["empty"])?, None);
    assert_eq!(excluded(&version, 1, &[])?, None);
    Ok(())
}

#[test]
fn unrecorded_folders_are_unknown_in_a_sealed_complete_or_missing_version() -> Result<(), String> {
    for (number, completeness) in [
        (2, ResultCompleteness::Complete),
        (3, ResultCompleteness::Missing),
    ] {
        let sink = IngestSink::default();
        let mut build = forest(&sink, 1, 1)?;
        dirs(&mut build, 0, &[&["a"], &["denied"]])?;
        gap(&mut build, 0, &["denied"], "scan.path_denied")?;
        let version = build.seal(id(number), completeness).map_err(err)?;
        assert_eq!(excluded(&version, 0, &["a"])?, None);
        assert_eq!(excluded(&version, 0, &[])?, None);
        // Unread entries cannot be shown to hold no excluded child.
        assert_eq!(excluded(&version, 0, &["denied"])?, None);
    }
    Ok(())
}

#[test]
fn undecided_and_other_folder_counts_are_omitted() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"], &["a", "nested"], &["b"]])?;
    for (parts, count) in [(&["a"][..], 3), (&["b"][..], 0)] {
        build
            .set_excluded_children(0, &folder_path(parts), count, true)
            .map_err(err)?;
    }
    gap(&mut build, 0, &["a", "unknown"], "scan.unclassified")?;
    gap(
        &mut build,
        0,
        &["a", "nested", "unknown"],
        "scan.unclassified",
    )?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(excluded(&version, 0, &["a"])?, None);
    assert_eq!(excluded(&version, 0, &["b"])?, None);
    assert_eq!(excluded(&version, 0, &["a", "nested"])?, None);
    Ok(())
}

#[test]
fn later_undecided_child_does_not_change_a_pinned_version() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"]])?;
    build
        .set_excluded_children(0, &folder_path(&["a"]), 1, true)
        .map_err(err)?;
    let interim = build.publish_partial(id(2)).map_err(err)?;
    gap(&mut build, 0, &["a", "unknown"], "scan.unclassified")?;
    let sealed = build
        .seal(id(3), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(excluded(&interim, 0, &["a"])?, None);
    assert_eq!(excluded(&sealed, 0, &["a"])?, None);
    Ok(())
}

#[test]
fn unreported_boundary_does_not_claim_an_exact_zero() -> Result<(), String> {
    for (visited, alias) in [("a", "b"), ("b", "a")] {
        let sink = IngestSink::default();
        let mut build = forest(&sink, 1, 1)?;
        dirs(&mut build, 0, &[&[visited], &[alias], &["volume"]])?;
        build
            .set_excluded_children(0, &folder_path(&[visited]), 1, true)
            .map_err(err)?;
        folder_support::unentered(&mut build, 0, &[alias], folder_support::REVISIT)?;
        folder_support::unentered(&mut build, 0, &["volume"], folder_support::OTHER_VOLUME)?;
        let interim = build.publish_partial(id(2)).map_err(err)?;
        let sealed = build
            .seal(id(3), ResultCompleteness::Complete)
            .map_err(err)?;
        for version in [&interim, &sealed] {
            assert_eq!(excluded(version, 0, &[visited])?, None);
            assert_eq!(excluded(version, 0, &[alias])?, None);
            assert_eq!(excluded(version, 0, &["volume"])?, None);
        }
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
fn partial_seal_omits_recorded_and_unrecorded_counts() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"], &["b"]])?;
    build
        .set_excluded_children(0, &folder_path(&["a"]), 1, true)
        .map_err(err)?;
    let version = build
        .seal(id(2), ResultCompleteness::Partial)
        .map_err(err)?;
    assert_eq!(excluded(&version, 0, &["a"])?, None);
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
