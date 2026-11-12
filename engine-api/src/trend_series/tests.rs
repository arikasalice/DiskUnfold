use super::*;
use du_snapshot_store::trend::{HistoryError, Observation, Point, PointEvidence};

fn id(index: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{index:012x}"))
}

fn snapshot(index: u32, at: i64, evidence: PointEvidence) -> Result<Observation, String> {
    Ok(Observation {
        source: ObservationSource::Snapshot(id(index)?),
        point: Point {
            observed_at_millis: at,
            evidence,
        },
    })
}

#[test]
fn deletion_is_bound_to_the_trend_revision_and_independent_of_zero() -> Result<(), String> {
    let revision = id(50)?;
    let points = vec![
        snapshot(1, 0, PointEvidence::Exact(5))?,
        snapshot(2, 0, PointEvidence::Exact(0))?,
        snapshot(3, 0, PointEvidence::AbsentComplete)?,
        snapshot(4, 0, PointEvidence::Exact(9))?,
        snapshot(5, 0, PointEvidence::Exact(8))?,
    ];
    let history =
        History::build(revision.clone(), points, 0).map_err(|error| format!("{error:?}"))?;
    let deleted = HashSet::from([id(3)?]);
    let none = HashSet::new();
    let project_slots = |revision, ids| {
        project(&history, DeletedSnapshots { revision, ids }, 2, None)
            .map(|output| output.slot_indices)
    };
    assert_eq!(project_slots(&revision, &none), Ok(vec![0, 1, 3, 4]));
    assert_eq!(project_slots(&revision, &deleted), Ok(vec![0, 1, 2, 3, 4]));
    let other = id(51)?;
    assert_eq!(
        project_slots(&other, &deleted),
        Err(Error::RevisionMismatch)
    );
    Ok(())
}

#[test]
fn actual_trend_points_keep_source_order_and_unverifiable_spans() -> Result<(), String> {
    let revision = id(52)?;
    let points = vec![
        snapshot(1, 0, PointEvidence::Exact(8))?,
        snapshot(2, 3, PointEvidence::UnknownSize)?,
        snapshot(3, 4, PointEvidence::Exact(6))?,
        Observation {
            source: ObservationSource::CurrentResult(id(9)?),
            point: Point {
                observed_at_millis: 0,
                evidence: PointEvidence::Exact(1),
            },
        },
    ];
    let history =
        History::build(revision.clone(), points, 2).map_err(|error| format!("{error:?}"))?;
    let deletes = HashSet::from([id(9)?]);
    let output = project(
        &history,
        DeletedSnapshots {
            revision: &revision,
            ids: &deletes,
        },
        2,
        None,
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(output.slot_indices, [0, 1, 2, 3]);
    assert_eq!(output.segment_unknown, [true, true, false]);
    assert_eq!(output.segment_original_point_count, [2, 2, 2]);
    assert!(output.decimated);
    Ok(())
}

#[test]
fn history_rejects_duplicate_snapshots_and_early_current_results() -> Result<(), String> {
    let revision = id(53)?;
    let point = snapshot(1, 0, PointEvidence::Exact(1))?;
    assert!(matches!(
        History::build(revision.clone(), vec![point.clone(), point.clone()], 0),
        Err(HistoryError::DuplicateSnapshot)
    ));
    let current = Observation {
        source: ObservationSource::CurrentResult(id(7)?),
        point: point.point,
    };
    assert!(matches!(
        History::build(revision, vec![current, point], 0),
        Err(HistoryError::CurrentResultNotLast)
    ));
    Ok(())
}
