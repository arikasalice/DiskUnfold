//! `space.total` reports the observation-time bounds of the items its total read.
use std::sync::Arc;

use du_analysis_index::{
    BuildHandle, IngestSink, Observation, ResultVersion,
    query::{self, Condition, Registry, page::ResultSets},
};
use du_core_contracts::{
    ByteSize, CallEnvelope, ENVELOPE_VERSION, Id, PathBytes, PathEncoding, SizeAmount, SizeBasis,
    SizeEvidence, SizeValue, Timestamp,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};
use du_engine_api::{
    api::{CallerCtx, Channel, Engine, EngineBuilder},
    space,
};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const T1: &str = "2026-11-18T01:00:00.000Z";
const T2: &str = "2026-11-18T02:00:00.000Z";
const T3: &str = "2026-11-18T03:00:00.000Z";
const T4: &str = "2026-11-18T04:00:00.000Z";
const T5: &str = "2026-11-18T05:00:00.000Z";

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn e<T: std::fmt::Debug>(error: T) -> String {
    format!("{error:?}")
}
fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}
fn root(name: &[u8], n: u8) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(name),
        },
        volume_id: vec![n],
        file_id: vec![n],
    }
}
/// Opens a forest whose roots carry the given observation times.
fn forest(store: &IngestSink, root_times: &[Option<&str>]) -> Result<BuildHandle, String> {
    let roots = (0..root_times.len() as u8)
        .map(|n| root(format!("/r{n}").as_bytes(), n + 1))
        .collect();
    let mut build = store.open_forest(id(1)?, roots).map_err(e)?;
    for (index, time) in root_times.iter().enumerate() {
        dir(&mut build, index as u32, &[], *time)?;
    }
    Ok(build)
}
fn observed(
    build: &mut BuildHandle,
    root: u32,
    parts: &[&str],
    time: Option<&str>,
) -> Result<(), String> {
    match time {
        Some(time) => build
            .set_item_observed_at(root, &path(parts), Timestamp::parse(time)?)
            .map_err(e),
        None => Ok(()),
    }
}
fn dir(
    build: &mut BuildHandle,
    root: u32,
    parts: &[&str],
    time: Option<&str>,
) -> Result<(), String> {
    build.add_directory_at(root, path(parts)).map_err(e)?;
    observed(build, root, parts, time)
}
fn file(
    build: &mut BuildHandle,
    root: u32,
    parts: &[&str],
    time: Option<&str>,
) -> Result<(), String> {
    build
        .add_file_at(
            root,
            Observation {
                path: path(parts),
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: ByteSize::from(1),
                    },
                }),
            },
        )
        .map_err(e)?;
    observed(build, root, parts, time)
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
fn folder(version: &ResultVersion, parts: &[&str]) -> Result<Value, String> {
    Ok(json!({
        "kind": "folder",
        "item": {"resultVersion": version.version_id(), "itemNo": item_no(version, parts)?}
    }))
}
fn engine_for(sets: Arc<ResultSets>) -> Result<Engine, Box<dyn std::error::Error>> {
    let mut builder = EngineBuilder::default();
    space::register_total_with_queries(&mut builder, sets);
    Ok(builder.build()?)
}
fn call_as(
    engine: &Engine,
    caller: &CallerCtx,
    target: Value,
    result_version: Option<&Id>,
) -> Result<Value, String> {
    let response = engine.call(
        CallEnvelope {
            name: "space.total".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: caller.correlation_id.clone(),
            body: json!({"target": target, "resultVersion": result_version, "requestSeq": 3}),
        },
        caller,
    );
    match response.error {
        Some(error) => Err(format!("{:?}", error.reason_key)),
        None => Ok(response.body),
    }
}
fn call(engine: &Engine, target: Value, result_version: Option<&Id>) -> Result<Value, String> {
    call_as(
        engine,
        &CallerCtx::new(Channel::Gui, id(200)?, None),
        target,
        result_version,
    )
}
fn range(earliest: Option<&str>, latest: Option<&str>, complete: bool) -> Value {
    json!({"earliest": earliest, "latest": latest, "complete": complete})
}
fn search(version: Arc<ResultVersion>, condition: Condition) -> Result<query::QueryResult, String> {
    query::run(
        &Registry::baseline(),
        version,
        &condition,
        query::Sort {
            id: "sort.path",
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        },
        || false,
    )
    .map_err(e)
}
fn everything() -> Condition {
    Condition::Not(Box::new(Condition::Any(vec![])))
}

/// Root 0 holds a, x and x/b; root 1 holds c. Times are added out of order.
fn two_roots(store: &IngestSink, b_time: Option<&str>) -> Result<BuildHandle, String> {
    let mut build = forest(store, &[Some(T3), Some(T1)])?;
    file(&mut build, 0, &["a"], Some(T5))?;
    dir(&mut build, 0, &["x"], Some(T2))?;
    file(&mut build, 0, &["x", "b"], b_time)?;
    file(&mut build, 1, &["c"], Some(T3))?;
    Ok(build)
}

#[test]
fn mixed_times_are_bounded_in_utc_order_across_roots_and_folders() -> TestResult {
    let store = IngestSink::default();
    let version = two_roots(&store, Some(T4))?
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(e)?;
    let engine = engine_for(Arc::new(ResultSets::new(store)))?;
    let scope = call(
        &engine,
        json!({"kind": "scope_root"}),
        Some(version.version_id()),
    )?;
    assert_eq!(scope["observationRange"], range(Some(T1), Some(T5), true));
    let root = call(&engine, folder(&version, &[])?, Some(version.version_id()))?;
    assert_eq!(root["observationRange"], range(Some(T2), Some(T5), true));
    let x = call(
        &engine,
        folder(&version, &["x"])?,
        Some(version.version_id()),
    )?;
    assert_eq!(x["observationRange"], range(Some(T2), Some(T4), true));
    Ok(())
}

#[test]
fn a_partial_range_keeps_its_confirmed_bounds_and_is_not_complete() -> TestResult {
    // A partial publication.
    let store = IngestSink::default();
    let mut build = two_roots(&store, Some(T4))?;
    let partial = build.publish_partial(id(2)?).map_err(e)?;
    let engine = engine_for(Arc::new(ResultSets::new(store)))?;
    let scope = call(
        &engine,
        json!({"kind": "scope_root"}),
        Some(partial.version_id()),
    )?;
    assert_eq!(scope["observationRange"], range(Some(T1), Some(T5), false));

    // A complete result with one item whose observation time is unknown.
    let store = IngestSink::default();
    let version = two_roots(&store, None)?
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(e)?;
    let engine = engine_for(Arc::new(ResultSets::new(store)))?;
    let x = call(
        &engine,
        folder(&version, &["x"])?,
        Some(version.version_id()),
    )?;
    assert_eq!(x["observationRange"], range(Some(T2), Some(T2), false));
    let scope = call(
        &engine,
        json!({"kind": "scope_root"}),
        Some(version.version_id()),
    )?;
    assert_eq!(scope["observationRange"], range(Some(T1), Some(T5), false));
    Ok(())
}

#[test]
fn a_query_total_reads_its_matched_items_and_an_empty_match_is_a_confirmed_empty_set() -> TestResult
{
    let store = IngestSink::default();
    let version = two_roots(&store, Some(T4))?
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(e)?;
    let sets = Arc::new(ResultSets::new(store));
    sets.register(
        id(3)?,
        search(Arc::clone(&version), Condition::Any(vec![]))?,
    )
    .map_err(e)?;
    sets.register(id(4)?, search(Arc::clone(&version), everything())?)
        .map_err(e)?;
    let engine = engine_for(sets)?;
    let empty = call(
        &engine,
        json!({"kind": "query_result", "queryRef": id(3)?}),
        None,
    )?;
    assert_eq!(empty["observationRange"], range(None, None, true));
    // Every item matches, so folders are in the set and the set range is not confirmed.
    let all = call(
        &engine,
        json!({"kind": "query_result", "queryRef": id(4)?}),
        None,
    )?;
    assert_eq!(all["observationRange"]["complete"], json!(false));
    Ok(())
}

#[test]
fn another_version_or_a_cancelled_request_returns_no_body() -> TestResult {
    let store = IngestSink::default();
    let version = two_roots(&store, Some(T4))?
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(e)?;
    let engine = engine_for(Arc::new(ResultSets::new(store)))?;
    let other = id(9)?;
    assert!(call(&engine, folder(&version, &["x"])?, Some(&other)).is_err());
    assert!(call(&engine, json!({"kind": "scope_root"}), Some(&other)).is_err());
    let caller = CallerCtx::new(Channel::Gui, id(201)?, None);
    caller.cancel();
    assert!(call_as(&engine, &caller, json!({"kind": "scope_root"}), None).is_err());
    Ok(())
}
