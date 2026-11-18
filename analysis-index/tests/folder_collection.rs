mod folder_support;

use du_analysis_index::{
    IngestSink, Observation,
    folder_collection::{
        BOUNDARY_ALREADY_COUNTED, BOUNDARY_OTHER_VOLUME, FolderBoundary, FolderCollection,
        REASON_DATA_INSUFFICIENT, REASON_IN_PROGRESS,
    },
};
use du_core_contracts::result::{ResultCompleteness, ResultVersionRead};
use folder_support::{
    OTHER_VOLUME, REVISIT, UNPROVEN, UNVERIFIABLE, bounded, dirs, err, forest, gap, id, state,
    unentered, unenumerated, unknown,
};

#[test]
fn unmarked_folders_of_a_sealed_complete_version_are_unknown() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 2)?;
    dirs(&mut build, 0, &[&["a"], &["a", "b"]])?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(state(&version, 0, &[])?, UNPROVEN);
    assert_eq!(state(&version, 0, &["a"])?, UNPROVEN);
    assert_eq!(state(&version, 0, &["a", "b"])?, UNPROVEN);
    assert_eq!(state(&version, 1, &[])?, UNPROVEN);
    Ok(())
}

#[test]
fn exact_gap_marks_the_folder_with_its_first_reason() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 2)?;
    dirs(&mut build, 0, &[&["a"], &["a", "b"], &["sibling"]])?;
    gap(&mut build, 0, &["a"], "scan.path_denied")?;
    gap(&mut build, 0, &["a"], "scan.disconnected")?;
    gap(&mut build, 1, &[], "scan.path_vanished")?;
    gap(&mut build, 0, &["a", "b"], UNVERIFIABLE)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(
        state(&version, 0, &["a"])?,
        unenumerated("scan.path_denied")
    );
    assert_eq!(state(&version, 0, &["a", "b"])?, unenumerated(UNVERIFIABLE));
    assert_eq!(state(&version, 1, &[])?, unenumerated("scan.path_vanished"));
    // A descendant's gap does not establish the ancestor's or sibling's listing state.
    assert_eq!(state(&version, 0, &[])?, UNPROVEN);
    assert_eq!(state(&version, 0, &["sibling"])?, UNPROVEN);
    // A gap turns a sealed Complete input into a Missing version.
    assert_eq!(version.completeness(), ResultCompleteness::Missing);
    Ok(())
}

#[test]
fn gap_on_a_name_that_is_not_an_item_does_not_mark_the_parent() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"]])?;
    gap(&mut build, 0, &["a", "undecidable"], "scan.unclassified")?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(version.scope_gaps().len(), 1);
    assert_eq!(state(&version, 0, &["a"])?, UNPROVEN);
    assert_eq!(state(&version, 0, &[])?, UNPROVEN);
    Ok(())
}

#[test]
fn boundary_issues_name_the_boundary_and_confirm_the_folder_was_not_read() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 2)?;
    dirs(
        &mut build,
        0,
        &[&["mnt"], &["again"], &["a"], &["a", "deep"]],
    )?;
    dirs(&mut build, 1, &[&["x"]])?;
    unentered(&mut build, 0, &["mnt"], OTHER_VOLUME)?;
    unentered(&mut build, 0, &["again"], REVISIT)?;
    unentered(&mut build, 0, &["a", "deep"], OTHER_VOLUME)?;
    unentered(&mut build, 1, &[], REVISIT)?;
    // An issue for a path that is no item marks nothing.
    unentered(&mut build, 0, &["ghost"], OTHER_VOLUME)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(
        state(&version, 0, &["mnt"])?,
        bounded(FolderBoundary::OtherVolume)
    );
    assert_eq!(
        state(&version, 0, &["again"])?,
        bounded(FolderBoundary::AlreadyCounted)
    );
    assert_eq!(
        state(&version, 0, &["a", "deep"])?,
        bounded(FolderBoundary::OtherVolume)
    );
    assert_eq!(
        state(&version, 1, &[])?,
        bounded(FolderBoundary::AlreadyCounted)
    );
    assert_eq!(state(&version, 0, &["a"])?, UNPROVEN);
    assert_eq!(state(&version, 1, &["x"])?, UNPROVEN);
    assert_eq!(
        FolderBoundary::OtherVolume.name(),
        Some(BOUNDARY_OTHER_VOLUME)
    );
    assert_eq!(
        FolderBoundary::AlreadyCounted.name(),
        Some(BOUNDARY_ALREADY_COUNTED)
    );
    assert_eq!(FolderBoundary::NoBoundary.name(), None);
    assert_eq!(
        (BOUNDARY_OTHER_VOLUME, BOUNDARY_ALREADY_COUNTED),
        ("other_volume", "already_counted")
    );
    Ok(())
}

#[test]
fn boundary_issues_are_reported_in_every_publication_and_completeness() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["mnt"], &["plain"]])?;
    unentered(&mut build, 0, &["mnt"], OTHER_VOLUME)?;
    let interim = build.publish_partial(id(2)).map_err(err)?;
    let cancelled = build
        .seal(id(3), ResultCompleteness::Cancelled)
        .map_err(err)?;
    for version in [&interim, &cancelled] {
        assert_eq!(
            state(version, 0, &["mnt"])?,
            bounded(FolderBoundary::OtherVolume)
        );
    }
    Ok(())
}

#[test]
fn gap_wins_over_a_boundary_issue_on_the_same_folder() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["both"], &["twice"]])?;
    unentered(&mut build, 0, &["both"], OTHER_VOLUME)?;
    gap(&mut build, 0, &["both"], "scan.path_denied")?;
    unentered(&mut build, 0, &["twice"], REVISIT)?;
    unentered(&mut build, 0, &["twice"], OTHER_VOLUME)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(
        state(&version, 0, &["both"])?,
        unenumerated("scan.path_denied")
    );
    // With both boundary kinds on one folder the answer is deterministic.
    assert_eq!(
        state(&version, 0, &["twice"])?,
        bounded(FolderBoundary::OtherVolume)
    );
    Ok(())
}

#[test]
fn unverifiable_boundary_is_an_unenumerated_gap_without_a_boundary() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["odd"]])?;
    unentered(&mut build, 0, &["odd"], UNVERIFIABLE)?;
    gap(&mut build, 0, &["odd"], UNVERIFIABLE)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(state(&version, 0, &["odd"])?, unenumerated(UNVERIFIABLE));
    Ok(())
}

#[test]
fn items_that_are_not_directories_are_not_a_folder() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    build
        .add_file_at(
            0,
            Observation {
                path: folder_support::folder_path(&["f"]),
                size: None,
            },
        )
        .map_err(err)?;
    build
        .add_untraversed_link_at(0, folder_support::folder_path(&["link"]))
        .map_err(err)?;
    build
        .add_at(
            0,
            Observation {
                path: folder_support::folder_path(&["odd"]),
                size: None,
            },
        )
        .map_err(err)?;
    build
        .add_structure(folder_support::folder_path(&["structure"]))
        .map_err(err)?;
    gap(&mut build, 0, &["f"], "scan.path_denied")?;
    unentered(&mut build, 0, &["link"], OTHER_VOLUME)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    for name in ["f", "link", "odd", "structure"] {
        assert_eq!(state(&version, 0, &[name])?, FolderCollection::NotAFolder);
    }
    Ok(())
}

#[test]
fn interim_publication_is_in_progress_and_the_seal_of_the_same_build_answers() -> Result<(), String>
{
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"], &["denied"]])?;
    gap(&mut build, 0, &["denied"], "scan.path_denied")?;
    let interim = build.publish_partial(id(2)).map_err(err)?;
    let sealed = build
        .seal(id(3), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(state(&interim, 0, &["a"])?, unknown(REASON_IN_PROGRESS));
    assert_eq!(state(&interim, 0, &[])?, unknown(REASON_IN_PROGRESS));
    // Recorded facts hold in every publication.
    assert_eq!(
        state(&interim, 0, &["denied"])?,
        unenumerated("scan.path_denied")
    );
    assert_eq!(state(&sealed, 0, &["a"])?, UNPROVEN);
    Ok(())
}

#[test]
fn partial_and_cancelled_seals_cannot_tell_an_unmarked_folder() -> Result<(), String> {
    for (number, completeness) in [
        (2, ResultCompleteness::Partial),
        (3, ResultCompleteness::Cancelled),
    ] {
        let sink = IngestSink::default();
        let mut build = forest(&sink, 1, 1)?;
        dirs(&mut build, 0, &[&["a"], &["denied"], &["mnt"]])?;
        gap(&mut build, 0, &["denied"], "scan.path_denied")?;
        unentered(&mut build, 0, &["mnt"], REVISIT)?;
        let version = build.seal(id(number), completeness).map_err(err)?;
        assert_eq!(
            state(&version, 0, &["a"])?,
            unknown(REASON_DATA_INSUFFICIENT)
        );
        assert_eq!(state(&version, 0, &[])?, unknown("space.data_insufficient"));
        assert_eq!(
            state(&version, 0, &["denied"])?,
            unenumerated("scan.path_denied")
        );
        assert_eq!(
            state(&version, 0, &["mnt"])?,
            bounded(FolderBoundary::AlreadyCounted)
        );
    }
    Ok(())
}

#[test]
fn sealed_missing_input_does_not_confirm_unmarked_folders() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"]])?;
    let version = build
        .seal(id(2), ResultCompleteness::Missing)
        .map_err(err)?;
    assert_eq!(state(&version, 0, &["a"])?, UNPROVEN);
    Ok(())
}

#[test]
fn completed_folder_record_does_not_prove_all_children_were_classified() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["read"], &["uncertain"], &["other"]])?;
    build
        .set_excluded_children(0, &folder_support::folder_path(&["read"]), 0, true)
        .map_err(err)?;
    build
        .set_excluded_children(0, &folder_support::folder_path(&["uncertain"]), 1, true)
        .map_err(err)?;
    gap(
        &mut build,
        0,
        &["uncertain", "undecidable"],
        "scan.unclassified",
    )?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(state(&version, 0, &["read"])?, UNPROVEN);
    assert_eq!(state(&version, 0, &["uncertain"])?, UNPROVEN);
    assert_eq!(state(&version, 0, &["other"])?, UNPROVEN);
    Ok(())
}
