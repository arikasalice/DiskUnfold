use super::*;
use crate::query::{self, Basis, Condition, Registry, Sort, page::ResultSets};

fn cached_search(
    sink: &IngestSink,
    version: Arc<crate::ResultVersion>,
    query_id: Id,
) -> Result<(ResultSets, std::sync::Weak<crate::query::QueryResult>), String> {
    let sets = ResultSets::new(sink.clone());
    let query = query::run(
        &Registry::baseline(),
        version,
        &Condition::All(Vec::new()),
        Sort {
            id: "sort.path",
            revision: 1,
            basis: Basis::Logical,
            descending: false,
        },
        || false,
    )
    .map_err(|error| format!("{error:?}"))?;
    sets.register(query_id.clone(), query)
        .map_err(|error| format!("{error:?}"))?;
    let cached = sets
        .open_search(&query_id, None)
        .map_err(|error| format!("{error:?}"))?;
    Ok((sets, Arc::downgrade(&cached)))
}

#[test]
fn snapshot_capacity_failure_publishes_without_evidence_and_retires_cached_result()
-> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = id(710)?;
    sink.bind_current_analysis("snapshot-without-baseline", lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    let mut build = sink
        .open_forest_in_lineage(id(711)?, vec![root()], lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 711)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, excluded(b"hidden".to_vec()))
        .map_err(|error| format!("{error:?}"))?;
    build
        .end_folder_attempt(token, 1, 1)
        .map_err(|error| format!("{error:?}"))?;
    let partial = build
        .publish_partial(id(712)?)
        .map_err(|error| format!("{error:?}"))?;
    let partial_items = partial.items().to_vec();
    let partial_summaries = partial
        .folder_attempt_summaries()
        .ok_or("partial attempt summaries are missing")?
        .to_vec();
    let (sets, cached) = cached_search(&sink, Arc::clone(&partial), id(715)?)?;
    let blocker = sink
        .folder_evidence_budget
        .reserve_remaining()
        .ok_or("reserve remaining global evidence budget failed")?;

    build
        .add_file(crate::Observation {
            path: vec![b"observed-after-budget".to_vec()],
            size: None,
        })
        .map_err(|error| format!("{error:?}"))?;
    let current = build
        .publish_partial(id(713)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(current.completeness(), ResultCompleteness::Partial);
    assert!(current.folder_attempt_summaries().is_none());
    assert!(
        current
            .items()
            .iter()
            .any(|item| item.name.as_slice() == b"observed-after-budget")
    );
    assert_eq!(partial.items(), partial_items.as_slice());
    assert_eq!(
        partial.folder_attempt_summaries().map(|rows| rows.to_vec()),
        Some(partial_summaries)
    );
    assert!(Arc::ptr_eq(
        &sink
            .pin_scan(Some(&lineage), None)
            .map_err(|error| format!("{error:?}"))?,
        &current
    ));
    assert!(matches!(
        sink.sealed_scan_result(partial.version_id()),
        Err(crate::SealedScanError::NotCurrent)
    ));
    assert!(matches!(
        sets.open_search(&id(715)?, None),
        Err(crate::query::page::SearchReadError::StaleVersion)
    ));
    assert!(cached.upgrade().is_none());
    drop(blocker);
    Ok(())
}

#[test]
fn replacement_snapshot_capacity_failure_publishes_without_evidence() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = id(720)?;
    sink.bind_current_analysis("replacement-snapshot-failure", lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    let mut base_build = sink
        .open_forest_in_lineage(id(721)?, vec![root()], lineage)
        .map_err(|error| format!("{error:?}"))?;
    base_build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let base = base_build
        .seal(id(722)?, ResultCompleteness::Partial)
        .map_err(|error| format!("{error:?}"))?;
    let base_items = base.items().to_vec();
    let (sets, cached) = cached_search(&sink, Arc::clone(&base), id(725)?)?;

    let mut replacement = sink
        .open_replacement(&base)
        .map_err(|error| format!("{error:?}"))?;
    replacement
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let token = replacement
        .begin_folder_attempt(0, &[], 722)
        .map_err(|error| format!("{error:?}"))?;
    replacement
        .record_folder_attempt_event(token, 0, excluded(b"hidden".to_vec()))
        .map_err(|error| format!("{error:?}"))?;
    replacement
        .end_folder_attempt(token, 1, 1)
        .map_err(|error| format!("{error:?}"))?;
    let blocker = sink
        .folder_evidence_budget
        .reserve_remaining()
        .ok_or("reserve remaining global evidence budget failed")?;

    let replacement_result = replacement
        .seal(id(723)?, ResultCompleteness::Partial)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        replacement_result.completeness(),
        ResultCompleteness::Partial
    );
    assert!(replacement_result.folder_attempt_summaries().is_none());
    assert_eq!(base.items(), base_items.as_slice());
    assert!(Arc::ptr_eq(
        &sink
            .sealed_scan_result(replacement_result.version_id())
            .map_err(|error| format!("{error:?}"))?,
        &replacement_result
    ));
    assert!(matches!(
        sets.open_search(&id(725)?, None),
        Err(crate::query::page::SearchReadError::StaleVersion)
    ));
    assert!(cached.upgrade().is_none());
    drop(blocker);
    Ok(())
}
