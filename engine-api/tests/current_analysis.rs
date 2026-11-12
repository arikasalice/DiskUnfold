use du_analysis_index::{IngestSink, browse};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, Timestamp,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use du_engine_api::current_analysis::{Error, admit_selected};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn publish(store: &IngestSink, lineage: &Id, scope: Id, version: Id) -> Result<(), String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = store
        .open_forest_in_lineage(scope, vec![root], lineage.clone())
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .seal(version, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

fn publish_empty_scope(
    store: &IngestSink,
    lineage: &Id,
    scope: Id,
    version: Id,
    completeness: ResultCompleteness,
) -> Result<(), String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/empty"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = store
        .open_forest_in_lineage(scope, vec![root], lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    build
        .set_item_observed_at(0, &[], Timestamp::parse("2026-11-12T00:00:00.000Z")?)
        .map_err(|error| format!("{error:?}"))?;
    build
        .seal(version, completeness)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

#[test]
fn selected_scope_is_pinned_without_using_other_lineages_latest() -> Result<(), String> {
    let store = IngestSink::default();
    assert_eq!(
        admit_selected(&store, &id(3)?, None).err(),
        Some(Error::NoSource)
    );
    let a = store
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = store
        .bind_current_analysis("b", id(4)?)
        .map_err(|e| format!("{e:?}"))?;
    publish(&store, &a, id(2)?, id(3)?)?;
    publish(&store, &b, id(5)?, id(6)?)?;
    let pinned = admit_selected(&store, &id(3)?, Some(&id(3)?)).map_err(|e| format!("{e:?}"))?;
    assert_eq!(pinned.lineage(), &a);
    assert_eq!(pinned.scope_version(), &id(2)?);
    assert_eq!(pinned.result_version(), &id(3)?);
    assert_eq!(
        admit_selected(&store, &id(3)?, Some(&id(6)?)).err(),
        Some(Error::StaleVersion)
    );
    assert_eq!(
        admit_selected(&store, &id(3)?, Some(&id(2)?)).err(),
        Some(Error::StaleVersion)
    );
    publish(&store, &a, id(7)?, id(8)?)?;
    assert_eq!(
        admit_selected(&store, &id(3)?, None).err(),
        Some(Error::StaleVersion)
    );
    assert_eq!(pinned.result_version(), &id(3)?);
    assert_eq!(pinned.scope_version(), &id(2)?);
    assert_eq!(
        admit_selected(&store, &id(8)?, None)
            .map_err(|e| format!("{e:?}"))?
            .lineage(),
        &a
    );
    Ok(())
}

#[test]
fn unbound_legacy_version_is_not_claimed_as_current_analysis() -> Result<(), String> {
    let store = IngestSink::default();
    let mut build = store.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        admit_selected(&store, &id(2)?, None).err(),
        Some(Error::UnsupportedSource)
    );
    Ok(())
}

#[test]
fn pinned_empty_scope_children_do_not_establish_observation_completeness() -> Result<(), String> {
    for (lineage_number, scope_number, version_number, completeness, expected_complete) in [
        (20, 21, 22, ResultCompleteness::Complete, true),
        (23, 24, 25, ResultCompleteness::Partial, false),
    ] {
        let store = IngestSink::default();
        let lineage = store
            .bind_current_analysis("empty-range", id(lineage_number)?)
            .map_err(|error| format!("bind lineage: {error:?}"))?;
        publish_empty_scope(
            &store,
            &lineage,
            id(scope_number)?,
            id(version_number)?,
            completeness,
        )?;
        let source = admit_selected(&store, &id(version_number)?, None)
            .map_err(|error| format!("pin empty result: {error:?}"))?;
        let scope = ItemRef {
            result_version: source.result_version().clone(),
            item_no: 0,
        };
        let observed = browse::open(source.version(), Some(&scope))
            .map_err(|error| format!("browse pinned empty scope: {error:?}"))?;
        assert!(observed.children.is_empty());
        let range = source
            .version()
            .observation_range(scope.item_no)
            .ok_or("empty scope has no observation range")?;
        assert_eq!(range.complete, expected_complete);
        assert!(range.min.is_some());
        assert!(range.max.is_some());
    }
    Ok(())
}
