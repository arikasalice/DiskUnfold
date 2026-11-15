mod folder_support;

use du_analysis_index::{
    BuildError, IngestSink, Observation,
    folder_collection::{
        FolderBoundary, FolderCollection, REASON_CANCELLED, REASON_DATA_INSUFFICIENT,
        REASON_DISCONNECTED, Unenumerated, UnreadFolderCounts, UnreadReason,
    },
};
use du_core_contracts::{
    ItemRef,
    result::{ResultCompleteness, ResultVersionRead},
};
use folder_support::{
    CONFIRMED_NO, OTHER_VOLUME, bounded, dirs, err, folder_path, forest, gap, id, state, unentered,
    unenumerated, unknown,
};

const CANCELLED: UnreadReason = UnreadReason::Cancelled;
const DISCONNECTED: UnreadReason = UnreadReason::Disconnected;

fn counts(cancelled: u32, disconnected: u32) -> UnreadFolderCounts {
    UnreadFolderCounts {
        cancelled,
        disconnected,
    }
}

#[test]
fn constants_and_reason_keys_are_fixed() {
    assert_eq!(REASON_CANCELLED, "scan.cancelled");
    assert_eq!(REASON_DISCONNECTED, "scan.disconnected");
    assert_eq!(CANCELLED.reason_key(), "scan.cancelled");
    assert_eq!(DISCONNECTED.reason_key(), "scan.disconnected");
    assert!(std::mem::size_of::<UnreadReason>() == 1);
}

#[test]
fn record_validation_matches_the_other_per_folder_records() -> Result<(), String> {
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
        build.set_directory_unread(0, &folder_path(&["dir"]), CANCELLED),
        Ok(())
    );
    assert_eq!(build.set_directory_unread(1, &[], DISCONNECTED), Ok(()));
    let invalid = Err(BuildError::InvalidTree);
    assert_eq!(
        build.set_directory_unread(0, &folder_path(&["file"]), CANCELLED),
        invalid
    );
    assert_eq!(
        build.set_directory_unread(0, &folder_path(&["none"]), CANCELLED),
        invalid
    );
    assert_eq!(build.set_directory_unread(2, &[], CANCELLED), invalid);
    assert_eq!(
        build.set_directory_unread(0, &[Vec::new()], CANCELLED),
        invalid
    );
    assert_eq!(
        build.set_directory_unread(1, &folder_path(&["dir"]), CANCELLED),
        invalid
    );
    // Rejected records leave the build usable and the accepted ones intact.
    let version = build
        .seal(id(2), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(version.unread_folder_counts(), counts(1, 1));
    Ok(())
}

#[test]
fn marked_folders_report_unenumerated_with_the_mark_reason() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 2)?;
    dirs(&mut build, 0, &[&["a"], &["a", "b"], &["c"], &["d"]])?;
    build
        .set_directory_unread(0, &folder_path(&["a", "b"]), CANCELLED)
        .map_err(err)?;
    build
        .set_directory_unread(0, &folder_path(&["c"]), DISCONNECTED)
        .map_err(err)?;
    build.set_directory_unread(1, &[], CANCELLED).map_err(err)?;
    let interim = build.publish_partial(id(2)).map_err(err)?;
    let sealed = build
        .seal(id(3), ResultCompleteness::Complete)
        .map_err(err)?;
    for version in [&interim, &sealed] {
        assert_eq!(
            state(version, 0, &["a", "b"])?,
            unenumerated("scan.cancelled")
        );
        assert_eq!(
            state(version, 0, &["c"])?,
            unenumerated("scan.disconnected")
        );
        assert_eq!(state(version, 1, &[])?, unenumerated("scan.cancelled"));
    }
    // Ancestors and siblings of a marked folder are not marked.
    assert_eq!(state(&sealed, 0, &["a"])?, CONFIRMED_NO);
    assert_eq!(state(&sealed, 0, &["d"])?, CONFIRMED_NO);
    assert_eq!(state(&interim, 0, &["d"])?, unknown("scan.in_progress"));
    Ok(())
}

#[test]
fn a_gap_wins_over_the_mark_and_the_mark_wins_over_a_boundary_issue() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["gapped"], &["mnt"], &["all"]])?;
    build
        .set_directory_unread(0, &folder_path(&["gapped"]), CANCELLED)
        .map_err(err)?;
    gap(&mut build, 0, &["gapped"], "scan.path_denied")?;
    build
        .set_directory_unread(0, &folder_path(&["mnt"]), DISCONNECTED)
        .map_err(err)?;
    unentered(&mut build, 0, &["mnt"], OTHER_VOLUME)?;
    build
        .set_directory_unread(0, &folder_path(&["all"]), CANCELLED)
        .map_err(err)?;
    unentered(&mut build, 0, &["all"], OTHER_VOLUME)?;
    gap(&mut build, 0, &["all"], "scan.path_vanished")?;
    let version = build
        .seal(id(2), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(
        state(&version, 0, &["gapped"])?,
        unenumerated("scan.path_denied")
    );
    assert_eq!(
        state(&version, 0, &["mnt"])?,
        unenumerated("scan.disconnected")
    );
    assert_eq!(
        state(&version, 0, &["all"])?,
        unenumerated("scan.path_vanished")
    );
    Ok(())
}

#[test]
fn the_invariant_holds_with_marks_gaps_and_boundaries_together() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"], &["b"], &["c"], &["d"]])?;
    for name in ["a", "b", "c"] {
        unentered(&mut build, 0, &[name], OTHER_VOLUME)?;
    }
    build
        .set_directory_unread(0, &folder_path(&["a"]), CANCELLED)
        .map_err(err)?;
    build
        .set_directory_unread(0, &folder_path(&["d"]), DISCONNECTED)
        .map_err(err)?;
    gap(&mut build, 0, &["b"], "scan.path_denied")?;
    let version = build
        .seal(id(2), ResultCompleteness::Cancelled)
        .map_err(err)?;
    let mut yes = 0;
    for entry in version.items() {
        let reference = ItemRef {
            result_version: version.version_id().clone(),
            item_no: entry.item_no,
        };
        if let FolderCollection::Folder {
            unenumerated,
            boundary,
        } = version.folder_collection(&reference).map_err(err)?
            && matches!(unenumerated, Unenumerated::Yes { .. })
        {
            yes += 1;
            assert_eq!(boundary, FolderBoundary::NoBoundary);
        }
    }
    assert_eq!(yes, 3);
    assert_eq!(
        state(&version, 0, &["c"])?,
        bounded(FolderBoundary::OtherVolume)
    );
    Ok(())
}

#[test]
fn a_later_record_replaces_the_earlier_one_in_future_publications() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = forest(&sink, 1, 1)?;
    dirs(&mut build, 0, &[&["a"]])?;
    build
        .set_directory_unread(0, &folder_path(&["a"]), CANCELLED)
        .map_err(err)?;
    let interim = build.publish_partial(id(2)).map_err(err)?;
    build
        .set_directory_unread(0, &folder_path(&["a"]), DISCONNECTED)
        .map_err(err)?;
    let sealed = build
        .seal(id(3), ResultCompleteness::Partial)
        .map_err(err)?;
    assert_eq!(state(&interim, 0, &["a"])?, unenumerated("scan.cancelled"));
    assert_eq!(
        state(&sealed, 0, &["a"])?,
        unenumerated("scan.disconnected")
    );
    assert_eq!(interim.unread_folder_counts(), counts(1, 0));
    assert_eq!(sealed.unread_folder_counts(), counts(0, 1));
    Ok(())
}

#[test]
fn cancelled_and_partial_seals_answer_for_marked_folders_only() -> Result<(), String> {
    for (number, completeness) in [
        (2, ResultCompleteness::Cancelled),
        (3, ResultCompleteness::Partial),
    ] {
        let sink = IngestSink::default();
        let mut build = forest(&sink, 1, 1)?;
        dirs(
            &mut build,
            0,
            &[&["read"], &["unread"], &["nested"], &["nested", "deep"]],
        )?;
        build
            .set_directory_unread(0, &folder_path(&["unread"]), CANCELLED)
            .map_err(err)?;
        build
            .set_directory_unread(0, &folder_path(&["nested", "deep"]), CANCELLED)
            .map_err(err)?;
        let version = build.seal(id(number), completeness).map_err(err)?;
        assert_eq!(
            state(&version, 0, &["unread"])?,
            unenumerated("scan.cancelled")
        );
        assert_eq!(
            state(&version, 0, &["nested", "deep"])?,
            unenumerated("scan.cancelled")
        );
        let insufficient = unknown(REASON_DATA_INSUFFICIENT);
        assert_eq!(state(&version, 0, &["read"])?, insufficient);
        assert_eq!(state(&version, 0, &["nested"])?, insufficient);
        assert_eq!(state(&version, 0, &[])?, insufficient);
    }
    Ok(())
}

#[test]
fn sample_versions_ignore_the_marks() -> Result<(), String> {
    use du_analysis_index::SampleSessionKey;
    let sink = IngestSink::default();
    let mut build = sink
        .open_sample_forest(
            id(1),
            vec![folder_support::root(b"/s")],
            &SampleSessionKey::new(id(9)),
        )
        .map_err(err)?;
    build.add_directory_at(0, Vec::new()).map_err(err)?;
    dirs(&mut build, 0, &[&["a"]])?;
    build
        .set_directory_unread(0, &folder_path(&["a"]), CANCELLED)
        .map_err(err)?;
    let version = build
        .seal(id(2), ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(
        state(&version, 0, &["a"])?,
        FolderCollection::Folder {
            unenumerated: Unenumerated::Unknown {
                reason_key: REASON_DATA_INSUFFICIENT
            },
            boundary: FolderBoundary::NotProvided,
        }
    );
    Ok(())
}
