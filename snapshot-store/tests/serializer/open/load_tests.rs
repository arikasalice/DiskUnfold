use super::*;
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    data_source::DataSource,
    result::{ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

fn live_result() -> Result<
    (
        du_analysis_index::IngestSink,
        Arc<du_analysis_index::ResultVersion>,
    ),
    String,
> {
    let sink = du_analysis_index::IngestSink::default();
    let scope =
        Id::parse("018f0000-0000-7000-8000-000000000031").map_err(|error| error.to_string())?;
    let version =
        Id::parse("018f0000-0000-7000-8000-000000000032").map_err(|error| error.to_string())?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/live"),
        },
        volume_id: b"live-volume".to_vec(),
        file_id: b"live-root".to_vec(),
    };
    let mut build = sink
        .open_forest(scope, vec![root])
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let current = build
        .seal(version, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Ok((sink, current))
}

#[test]
fn empty_written_snapshot_loads_as_an_immutable_result() -> Result<(), String> {
    let source = empty_result(None)?;
    let bytes = write_snapshot(
        &source,
        &source_metadata(true, source.observed_roots().len()),
    )?;
    let (snapshot, _) = open_bytes(bytes)?;
    let (live, current) = live_result()?;
    let before = live
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("live result missing")?;
    let loaded =
        du_snapshot_store::load::load(snapshot, || false).map_err(|error| error.to_string())?;
    let (expected_snapshot_id, _) = identity()?;

    assert_eq!(loaded.data_source(), DataSource::Snapshot);
    assert_eq!(loaded.snapshot_id(), Some(&expected_snapshot_id));
    assert_eq!(loaded.version_id(), source.version_id());
    assert_eq!(loaded.lineage_id(), None);
    assert_eq!(loaded.completeness(), source.completeness());
    assert_eq!(loaded.observed_at(), source.observed_at());
    assert_eq!(loaded.observed_roots(), source.observed_roots());
    assert_eq!(loaded.items().len(), 1);
    assert!(loaded.scope_gaps().is_empty());

    let after = live
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("live result missing after load")?;
    assert!(Arc::ptr_eq(&current, &before));
    assert!(Arc::ptr_eq(&current, &after));
    Ok(())
}

#[test]
fn multiroot_load_keeps_gaps_and_unknown_root_identity() -> Result<(), String> {
    let source = source_result(ResultCompleteness::Partial)?;
    let bytes = write_snapshot(
        &source,
        &source_metadata(false, source.observed_roots().len()),
    )?;
    let (snapshot, _) = open_bytes(bytes.clone())?;
    let (live, current) = live_result()?;
    let before = live
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("live result missing")?;
    let loaded =
        du_snapshot_store::load::load(snapshot, || false).map_err(|error| error.to_string())?;
    let (expected_snapshot_id, _) = identity()?;

    assert_eq!(loaded.data_source(), DataSource::Snapshot);
    assert_eq!(loaded.snapshot_id(), Some(&expected_snapshot_id));
    assert_eq!(loaded.version_id(), source.version_id());
    assert_eq!(loaded.lineage_id(), None);
    assert_eq!(loaded.completeness(), source.completeness());
    assert_eq!(loaded.observed_at(), source.observed_at());
    assert_eq!(loaded.observed_roots(), source.observed_roots());
    assert_eq!(loaded.items().len(), source.items().len());
    assert_eq!(loaded.scope_gaps(), source.scope_gaps());
    assert_eq!(loaded.volume_reports(), source.volume_reports());

    let volume_body = decoded(&bytes)?
        .records
        .into_iter()
        .find(|record| record.0 == 1)
        .ok_or("Volume record missing")?;
    let links = records::volume::decode(&volume_body.1, 3)
        .map_err(|error| format!("{error:?}"))?
        .roots;
    let root_reference = |root_index: usize| -> Result<du_core_contracts::ItemRef, String> {
        let item_no = links
            .get(root_index)
            .and_then(|link| link.item_number)
            .and_then(|number| number.checked_sub(1))
            .ok_or("root item missing")?;
        Ok(du_core_contracts::ItemRef {
            result_version: loaded.version_id().clone(),
            item_no,
        })
    };
    assert!(
        loaded
            .unread_inside(&root_reference(0)?)
            .map_err(|error| format!("{error:?}"))?
            .is_none()
    );
    assert!(matches!(
        loaded
            .unread_inside(&root_reference(1)?)
            .map_err(|error| format!("{error:?}"))?,
        Some(du_analysis_index::folder_collection::UnreadInside::Recorded { count: Some(1) })
    ));
    let unknown = loaded.observed_roots().get(2).ok_or("third root missing")?;
    assert!(unknown.volume_id.is_empty());
    assert_eq!(unknown.file_id, [33]);

    assert!(Arc::ptr_eq(&current, &before));
    let after = live
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("live result missing after load")?;
    assert!(Arc::ptr_eq(&current, &after));
    Ok(())
}

#[test]
fn cancelled_load_returns_nothing_and_leaves_live_current_unchanged() -> Result<(), String> {
    let source = empty_result(None)?;
    let bytes = write_snapshot(
        &source,
        &source_metadata(true, source.observed_roots().len()),
    )?;
    let (snapshot, _) = open_bytes(bytes)?;
    let (live, current) = live_result()?;
    let before = live
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("live result missing")?;
    let mut polls = 0;
    let result = du_snapshot_store::load::load(snapshot, || {
        polls += 1;
        polls > 4
    });
    assert!(matches!(
        result,
        Err(du_snapshot_store::load::LoadError::Cancelled)
    ));
    let after = live
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("live result missing after cancellation")?;
    assert!(Arc::ptr_eq(&current, &before));
    assert!(Arc::ptr_eq(&current, &after));
    Ok(())
}
