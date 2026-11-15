//! `space.total` reports the size rule and the cancelled-unread folder count of its target.
use std::sync::Arc;

use du_analysis_index::{
    IngestSink, ResultVersion,
    folder_collection::UnreadReason,
    query::{self, Condition, Registry, page::ResultSets},
};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id,
    result::{ResultCompleteness, ResultVersionRead},
};
use du_engine_api::{
    api::{CallerCtx, Channel, Engine, EngineBuilder},
    space,
};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}

fn item_no(version: &ResultVersion, parts: &[&str]) -> Result<u32, String> {
    let mut current = 0;
    for part in parts {
        current = version
            .items()
            .iter()
            .find(|item| item.parent == Some(current) && item.name == part.as_bytes())
            .ok_or("missing item")?
            .item_no;
    }
    Ok(current)
}

/// Builds a tree with a, a/b, a/b/c and d, marks a/b as cancelled and d as disconnected.
fn marked_build(store: &IngestSink) -> Result<du_analysis_index::BuildHandle, String> {
    let mut build = store.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    for parts in [&[][..], &["a"], &["a", "b"], &["a", "b", "c"], &["d"]] {
        build
            .add_directory(path(parts))
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .set_directory_unread(0, &path(&["a", "b"]), UnreadReason::Cancelled)
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_directory_unread(0, &path(&["d"]), UnreadReason::Disconnected)
        .map_err(|e| format!("{e:?}"))?;
    Ok(build)
}

fn engine_for(sets: Arc<ResultSets>) -> Result<Engine, Box<dyn std::error::Error>> {
    let mut builder = EngineBuilder::default();
    space::register_total_with_queries(&mut builder, sets);
    Ok(builder.build()?)
}

fn call(engine: &Engine, target: Value, result_version: Option<&Id>) -> Result<Value, String> {
    let caller = CallerCtx::new(Channel::Gui, id(200)?, None);
    let response = engine.call(
        CallEnvelope {
            name: "space.total".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: caller.correlation_id.clone(),
            body: json!({"target": target, "resultVersion": result_version, "requestSeq": 3}),
        },
        &caller,
    );
    match response.error {
        Some(error) => Err(format!("{:?}", error.reason_key)),
        None => Ok(response.body),
    }
}

fn folder(version: &ResultVersion, parts: &[&str]) -> Result<Value, String> {
    Ok(json!({
        "kind": "folder",
        "item": {"resultVersion": version.version_id(), "itemNo": item_no(version, parts)?}
    }))
}

#[test]
fn a_sealed_scan_reports_the_rule_and_the_count_of_every_subtree_target() -> TestResult {
    let store = IngestSink::default();
    let version = marked_build(&store)?
        .seal(id(2)?, ResultCompleteness::Cancelled)
        .map_err(|e| format!("{e:?}"))?;
    let engine = engine_for(Arc::new(ResultSets::new(store)))?;
    let rule = json!({"id": "size.agg.subtree-unique", "revision": 1});

    let scope = call(&engine, json!({"kind": "scope_root"}), None)?;
    assert_eq!(scope["sizeRule"], rule);
    assert_eq!(scope["cancelledUnreadFolders"], 1);
    assert_eq!(
        scope["cancelledUnreadFolders"],
        json!(version.unread_folder_counts().cancelled)
    );
    for (parts, expected) in [
        (&[][..], 1),
        (&["a"], 1),
        (&["a", "b"], 1),
        (&["a", "b", "c"], 0),
        (&["d"], 0),
    ] {
        let body = call(
            &engine,
            folder(&version, parts)?,
            Some(version.version_id()),
        )?;
        assert_eq!(body["sizeRule"], rule, "{parts:?}");
        assert_eq!(body["cancelledUnreadFolders"], expected, "{parts:?}");
    }
    Ok(())
}

#[test]
fn a_sealed_scan_that_was_not_cancelled_confirms_zero() -> TestResult {
    let store = IngestSink::default();
    let mut build = store.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(path(&[]))
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let engine = engine_for(Arc::new(ResultSets::new(store)))?;
    let body = call(&engine, json!({"kind": "scope_root"}), None)?;
    assert_eq!(body.get("cancelledUnreadFolders"), Some(&json!(0)));
    assert!(body.get("sizeRule").is_some());
    Ok(())
}

#[test]
fn a_cancelled_scan_without_collector_marks_omits_the_count() -> TestResult {
    let store = IngestSink::default();
    let mut build = store.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    for parts in [&[][..], &["a"]] {
        build
            .add_directory(path(parts))
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Cancelled)
        .map_err(|e| format!("{e:?}"))?;
    let engine = engine_for(Arc::new(ResultSets::new(store)))?;
    for target in [json!({"kind": "scope_root"}), folder(&version, &["a"])?] {
        let body = call(&engine, target, Some(version.version_id()))?;
        assert!(body.get("sizeRule").is_some());
        assert_eq!(body.get("cancelledUnreadFolders"), None);
    }
    Ok(())
}

#[test]
fn an_unfinished_publication_omits_the_count_and_keeps_the_rule() -> TestResult {
    let store = IngestSink::default();
    let mut build = marked_build(&store)?;
    let interim = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    let engine = engine_for(Arc::new(ResultSets::new(store)))?;
    let rule = json!({"id": "size.agg.subtree-unique", "revision": 1});
    for target in [json!({"kind": "scope_root"}), folder(&interim, &["a"])?] {
        let body = call(&engine, target, Some(interim.version_id()))?;
        assert_eq!(body["sizeRule"], rule);
        assert_eq!(body.get("cancelledUnreadFolders"), None);
    }
    Ok(())
}

#[test]
fn a_query_result_target_reports_the_rule_without_a_count() -> TestResult {
    let store = IngestSink::default();
    let version = marked_build(&store)?
        .seal(id(2)?, ResultCompleteness::Cancelled)
        .map_err(|e| format!("{e:?}"))?;
    let selected = query::run(
        &Registry::baseline(),
        version,
        &Condition::All(vec![]),
        query::Sort {
            id: "sort.path",
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        },
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let sets = Arc::new(ResultSets::new(store));
    sets.register(id(3)?, selected)
        .map_err(|e| format!("{e:?}"))?;
    let engine = engine_for(sets)?;
    let body = call(
        &engine,
        json!({"kind": "query_result", "queryRef": id(3)?}),
        None,
    )?;
    assert_eq!(
        body["sizeRule"],
        json!({"id": "size.agg.subtree-unique", "revision": 1})
    );
    assert_eq!(body.get("cancelledUnreadFolders"), None);
    Ok(())
}
