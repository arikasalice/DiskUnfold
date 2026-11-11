//! Source evidence only: these checks do not authorize a share or define a scope contract.
use du_analysis_index::{IngestSink, QueryError};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use du_engine_api::current_analysis::{Error, admit_selected};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn publish(
    store: &IngestSink,
    lineage: &Id,
    scope: Id,
    version: Id,
    volume: u8,
    root_bytes: &[u8],
) -> Result<(), String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(root_bytes),
        },
        volume_id: vec![volume],
        file_id: vec![volume],
    };
    let mut build = store
        .open_forest_in_lineage(scope, vec![root], lineage.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![b"child".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(version, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[test]
fn pinned_source_retains_its_own_item_path_and_volume_after_replacement() -> Result<(), String> {
    let store = IngestSink::default();
    let lineage = store
        .bind_current_analysis("fixture", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    publish(&store, &lineage, id(2)?, id(3)?, 1, b"/first")?;
    let old = admit_selected(&store, &id(3)?, None).map_err(|e| format!("{e:?}"))?;
    let old_item = ItemRef {
        result_version: id(3)?,
        item_no: 1,
    };
    let (_, old_root) = old
        .version()
        .observed_root_for(&old_item)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(old_root.volume_id, [1]);
    assert_eq!(
        old.version()
            .native_path(&old_item)
            .map_err(|e| format!("{e:?}"))?
            .raw
            .bytes()
            .map_err(|e| e.to_string())?,
        b"/first/child"
    );

    publish(&store, &lineage, id(4)?, id(5)?, 2, b"/second")?;
    let new = admit_selected(&store, &id(5)?, None).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        admit_selected(&store, &id(3)?, None).err(),
        Some(Error::StaleVersion)
    );
    assert_eq!(
        new.version().native_path(&old_item),
        Err(QueryError::StaleVersion)
    );
    assert_eq!(
        new.version().observed_root_for(&old_item).err(),
        Some(QueryError::StaleVersion)
    );
    assert_eq!(
        old.version()
            .native_path(&old_item)
            .map_err(|e| format!("{e:?}"))?
            .raw
            .bytes()
            .map_err(|e| e.to_string())?,
        b"/first/child"
    );
    let new_item = ItemRef {
        result_version: id(5)?,
        item_no: 1,
    };
    let (_, new_root) = new
        .version()
        .observed_root_for(&new_item)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(new_root.volume_id, [2]);
    assert_eq!(
        new.version()
            .native_path(&new_item)
            .map_err(|e| format!("{e:?}"))?
            .raw
            .bytes()
            .map_err(|e| e.to_string())?,
        b"/second/child"
    );
    Ok(())
}
