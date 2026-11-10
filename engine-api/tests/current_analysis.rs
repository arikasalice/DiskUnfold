use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
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
