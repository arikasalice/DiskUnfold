use super::*;
use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id,
    result::ResultCompleteness,
    scan_observation::{ObservedValue, attribute_reason},
};

fn build() -> Result<BuildHandle, String> {
    let id = Id::parse("018f0000-0000-7000-8000-000000000001").map_err(|e| format!("{e:?}"))?;
    IngestSink::default().open(id).map_err(|e| format!("{e:?}"))
}

fn fail(build: &mut BuildHandle, counts: &mut JobCounts, counted: Counted) -> Result<(), String> {
    let range = (0, vec![b"dir".to_vec()]);
    let failure = (ScanFailureStage::Enumerate, "scan.path_denied");
    record_failure(build, counts, range, failure, (None, None), counted)
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn a_failure_moves_one_completed_count_to_failed() -> Result<(), String> {
    let mut build = build()?;
    let mut counts = JobCounts {
        completed: 2,
        ..JobCounts::default()
    };
    fail(&mut build, &mut counts, Counted::Completed)?;
    assert_eq!((counts.completed, counts.failed), (1, 1));
    fail(&mut build, &mut counts, Counted::Failed)?;
    assert_eq!((counts.completed, counts.failed), (1, 1));
    Ok(())
}

/// A second failure of a range finds no completed count to move.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a failed range must have been counted as completed first")]
fn a_move_without_a_completed_count_stops_debug_builds() {
    let Ok(mut build) = build() else {
        return;
    };
    let mut counts = JobCounts::default();
    let _ = fail(&mut build, &mut counts, Counted::Completed);
}

#[test]
fn roots_fail_once_and_start_as_their_admission_counted_them() {
    let mut ranges = FailedRanges::new(vec![false, true]);
    assert_eq!(ranges.counted(0, &[]), Counted::Completed);
    assert_eq!(ranges.counted(0, &[]), Counted::Failed);
    // An unverifiable root was counted as failed when it was admitted.
    assert_eq!(ranges.counted(1, &[]), Counted::Failed);
    assert_eq!(ranges.counted(0, &[b"a".to_vec()]), Counted::Completed);
    assert_eq!(ranges.counted(1, &[b"a".to_vec()]), Counted::Completed);
}

/// A second failure of one path would move another item's completed count.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a range below a root failed twice")]
fn a_second_failure_of_one_path_stops_debug_builds() {
    let mut ranges = FailedRanges::new(vec![false]);
    let path = [b"a".to_vec()];
    let _ = ranges.counted(0, &path);
    let _ = ranges.counted(0, &path);
}

#[test]
fn excluded_counts_above_the_reported_range_saturate_as_a_lower_bound() {
    assert_eq!(excluded_children(0, true), None);
    assert_eq!(excluded_children(3, true), Some((3, true)));
    assert_eq!(excluded_children(3, false), Some((3, false)));
    let max = u64::from(u32::MAX);
    assert_eq!(excluded_children(max, true), Some((u32::MAX, true)));
    assert_eq!(excluded_children(max + 1, true), Some((u32::MAX, false)));
}

/// A folder is never marked twice, nor both failed and marked.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a directory with a failed range was marked as unread")]
fn a_failed_folder_that_is_marked_stops_debug_builds() {
    let mut ranges = FailedRanges::new(vec![false]);
    let path = [b"a".to_vec()];
    let _ = ranges.counted(0, &path);
    ranges.unread(0, &path);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a directory was marked as unread twice")]
fn a_second_mark_of_one_folder_stops_debug_builds() {
    let mut ranges = FailedRanges::new(vec![false]);
    ranges.unread(0, &[]);
    ranges.unread(0, &[]);
}

/// FR2.HD1 stage 2 (#239): a recorded entry keeps why each attribute is unknown.
#[test]
fn a_recorded_file_keeps_the_reason_of_each_unknown_attribute() -> Result<(), String> {
    let mut build = build()?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let entry = ObservedEntry {
        name: b"f".to_vec(),
        enumerated_inode: 7,
        enumerated_kind: ObservedKind::File,
        metadata: Ok(MetadataObservation {
            kind: ObservedKind::File,
            device: 1,
            inode: 7,
            size: Ok(3),
            modified_at: ObservedValue::Unknown(attribute_reason::QUERY_FAILED),
            native_modified_at: None,
            allocated: ObservedValue::Unknown(attribute_reason::PLACEHOLDER),
            link_count: ObservedValue::Known(1),
            space_identity: ObservedValue::Unknown(attribute_reason::UNSUPPORTED),
        }),
    };
    let mut counts = JobCounts::default();
    let path = [b"f".to_vec()];
    record_at(
        &mut build,
        0,
        &entry,
        &path,
        &mut counts,
        (
            None,
            SharingEvidence::None,
            super::super::admission::Admission::Record,
        ),
    )
    .map_err(|e| format!("{e:?}"))?;
    let id = Id::parse("018f0000-0000-7000-8000-000000000002").map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let reason = |attribute| version.attribute_unknown_reason(1, attribute);
    assert_eq!(
        reason(ObservedAttribute::ModifiedAt),
        Some(attribute_reason::QUERY_FAILED)
    );
    assert_eq!(
        reason(ObservedAttribute::Allocated),
        Some(attribute_reason::PLACEHOLDER)
    );
    assert_eq!(reason(ObservedAttribute::LinkCount), None);
    assert_eq!(
        reason(ObservedAttribute::SpaceIdentity),
        Some(attribute_reason::UNSUPPORTED)
    );
    assert_eq!(version.attribute_reason_bytes(), 15);
    Ok(())
}
