use super::*;
use du_snapshot_store::trend::{
    Observation, ObservationSource, Point, PointEvidence as InputEvidence,
};
use std::collections::HashSet;

fn id(index: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{index:012x}"))
}

fn observation(index: u32, at: i64, evidence: InputEvidence) -> Result<Observation, String> {
    Ok(Observation {
        source: ObservationSource::Snapshot(id(index)?),
        point: Point {
            observed_at_millis: at,
            evidence,
        },
    })
}

fn deletions<'a>(revision: &'a Id, ids: &'a HashSet<Id>) -> DeletedSnapshots<'a> {
    DeletedSnapshots { revision, ids }
}

#[test]
fn point_evidence_keeps_source_status_future_and_revision_bound_deletion() -> Result<(), String> {
    let revision = id(50)?;
    let snapshot_id = id(1)?;
    let current_id = id(2)?;
    let history = History::build(
        revision.clone(),
        vec![
            observation(1, 0, InputEvidence::UnknownSize)?,
            Observation {
                source: ObservationSource::CurrentResult(current_id.clone()),
                point: Point {
                    observed_at_millis: 2,
                    evidence: InputEvidence::Exact(8),
                },
            },
        ],
        1,
    )
    .map_err(|error| format!("{error:?}"))?;
    let deleted_ids = HashSet::from([snapshot_id.clone(), current_id.clone()]);

    let past = assemble(
        &history,
        &revision,
        deletions(&revision, &deleted_ids),
        2,
        None,
        Target::Point { slot_index: 0 },
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(past.revision, revision);
    assert!(past.guidance_keys.contains(&guidance::UNVERIFIABLE));
    assert!(past.guidance_keys.contains(&guidance::CAUSE_NOT_OBSERVED));
    assert!(matches!(
        past.target,
        EvidenceTarget::Point(PointEvidence {
            source_kind: SourceKind::HistoricalObservation,
            source_id,
            deleted: true,
            reason_key: Some("trend.point.size_unknown"),
            ..
        }) if source_id == snapshot_id
    ));

    let current = assemble(
        &history,
        &revision,
        deletions(&revision, &deleted_ids),
        2,
        None,
        Target::Point { slot_index: 1 },
    )
    .map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        current.target,
        EvidenceTarget::Point(PointEvidence {
            source_kind: SourceKind::CurrentResult,
            source_id,
            deleted: false,
            flags,
            ..
        }) if source_id == current_id && flags.contains(&Flag::OrderUncertain)
    ));
    assert!(current.guidance_keys.contains(&guidance::ORDER_UNCERTAIN));
    Ok(())
}

#[test]
fn projected_segment_expands_back_to_each_original_adjacent_pair() -> Result<(), String> {
    let revision = id(51)?;
    let observations = (0..6)
        .map(|index| {
            observation(
                index + 1,
                i64::from(index),
                InputEvidence::Exact(u64::from(index) + 1),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let history =
        History::build(revision.clone(), observations, 5).map_err(|error| format!("{error:?}"))?;
    let deleted_ids = HashSet::new();
    let evidence = assemble(
        &history,
        &revision,
        deletions(&revision, &deleted_ids),
        2,
        None,
        Target::Segment {
            first_slot: 0,
            last_slot: 2,
        },
    )
    .map_err(|error| format!("{error:?}"))?;

    let EvidenceTarget::Segment(projected) = evidence.target else {
        return Err("expected projected segment evidence".to_owned());
    };
    assert!(projected.last_slot - projected.first_slot > 1);
    assert_eq!(
        projected.original_point_count,
        projected.last_slot - projected.first_slot + 1
    );
    assert_eq!(projected.segments.len(), projected.original_point_count - 1);
    for (offset, segment) in projected.segments.iter().enumerate() {
        let expected_before = projected.first_slot + offset;
        assert_eq!(segment.before.source_id, id(expected_before as u32 + 1)?);
        assert_eq!(segment.after.source_id, id(expected_before as u32 + 2)?);
        assert_eq!(segment.status, SegmentStatus::Comparable);
        assert_eq!(segment.delta_bytes, Some(1));
    }
    assert!(!projected.has_unverifiable_segment);
    assert!(evidence.guidance_keys.contains(&guidance::OBSERVED_DELTA));
    assert!(
        evidence
            .guidance_keys
            .contains(&guidance::CAUSE_NOT_OBSERVED)
    );
    assert_eq!(
        assemble(
            &history,
            &revision,
            deletions(&revision, &deleted_ids),
            2000,
            None,
            Target::Segment {
                first_slot: 0,
                last_slot: 2,
            },
        ),
        Err(Error::SegmentNotFound)
    );
    Ok(())
}

#[test]
fn projected_unknown_segment_does_not_claim_an_observed_delta() -> Result<(), String> {
    let revision = id(52)?;
    let history = History::build(
        revision.clone(),
        vec![
            observation(1, 0, InputEvidence::Exact(1))?,
            observation(2, 1, InputEvidence::Exact(5))?,
            observation(3, 2, InputEvidence::UnknownSize)?,
            observation(4, 3, InputEvidence::Exact(10))?,
            observation(5, 4, InputEvidence::Exact(4))?,
            observation(6, 5, InputEvidence::Exact(5))?,
            observation(7, 6, InputEvidence::Exact(6))?,
            observation(8, 7, InputEvidence::Exact(7))?,
        ],
        1,
    )
    .map_err(|error| format!("{error:?}"))?;
    let deleted_ids = HashSet::new();
    let evidence = assemble(
        &history,
        &revision,
        deletions(&revision, &deleted_ids),
        2,
        None,
        Target::Segment {
            first_slot: 0,
            last_slot: 2,
        },
    )
    .map_err(|error| format!("{error:?}"))?;

    let EvidenceTarget::Segment(projected) = evidence.target else {
        return Err("expected projected segment evidence".to_owned());
    };
    assert!(projected.has_unverifiable_segment);
    assert!(projected.original_point_count > 2);
    assert_eq!(projected.segments[0].status, SegmentStatus::Comparable);
    assert_eq!(projected.segments[1].status, SegmentStatus::Unknown);
    assert!(evidence.guidance_keys.contains(&guidance::UNVERIFIABLE));
    assert!(!evidence.guidance_keys.contains(&guidance::OBSERVED_DELTA));
    assert!(evidence.guidance_keys.contains(&guidance::ORDER_UNCERTAIN));
    assert!(
        evidence
            .guidance_keys
            .contains(&guidance::CAUSE_NOT_OBSERVED)
    );
    let changed_deletions = HashSet::from([id(2)?]);
    assert_eq!(
        assemble(
            &history,
            &revision,
            deletions(&revision, &changed_deletions),
            2,
            None,
            Target::Segment {
                first_slot: 0,
                last_slot: 2,
            },
        ),
        Err(Error::SegmentNotFound)
    );
    Ok(())
}

#[test]
fn mismatched_deletion_revision_and_missing_projection_references_fail_closed() -> Result<(), String>
{
    let revision = id(53)?;
    let history = History::build(
        revision.clone(),
        vec![observation(1, 0, InputEvidence::Exact(1))?],
        0,
    )
    .map_err(|error| format!("{error:?}"))?;
    let deleted_ids = HashSet::new();
    let other_revision = id(54)?;
    assert_eq!(
        assemble(
            &history,
            &revision,
            deletions(&other_revision, &deleted_ids),
            2,
            None,
            Target::Point { slot_index: 0 },
        ),
        Err(Error::Projection(trend_series::Error::RevisionMismatch))
    );
    let other_history = History::build(
        other_revision.clone(),
        vec![observation(1, 0, InputEvidence::Exact(1))?],
        0,
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        assemble(
            &other_history,
            &revision,
            deletions(&other_revision, &deleted_ids),
            2,
            None,
            Target::Point { slot_index: 0 },
        ),
        Err(Error::RevisionMismatch)
    );
    assert_eq!(
        assemble(
            &history,
            &revision,
            deletions(&revision, &deleted_ids),
            2,
            None,
            Target::Point { slot_index: 1 },
        ),
        Err(Error::PointNotFound)
    );
    assert_eq!(
        assemble(
            &history,
            &revision,
            deletions(&revision, &deleted_ids),
            2,
            None,
            Target::Segment {
                first_slot: 0,
                last_slot: 1,
            },
        ),
        Err(Error::SegmentNotFound)
    );
    Ok(())
}

#[test]
fn single_unknown_point_keeps_both_applicable_explanation_keys() -> Result<(), String> {
    let revision = id(55)?;
    let history = History::build(
        revision.clone(),
        vec![observation(1, 0, InputEvidence::Missing)?],
        0,
    )
    .map_err(|error| format!("{error:?}"))?;
    let deleted_ids = HashSet::new();
    let evidence = assemble(
        &history,
        &revision,
        deletions(&revision, &deleted_ids),
        2,
        None,
        Target::Point { slot_index: 0 },
    )
    .map_err(|error| format!("{error:?}"))?;
    assert!(
        evidence
            .guidance_keys
            .contains(&guidance::SINGLE_OBSERVATION)
    );
    assert!(evidence.guidance_keys.contains(&guidance::UNVERIFIABLE));
    assert!(
        evidence
            .guidance_keys
            .contains(&guidance::CAUSE_NOT_OBSERVED)
    );
    Ok(())
}
