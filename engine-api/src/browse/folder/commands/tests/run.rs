//! query.run over a subtree through the registered GUI command.

use super::*;
use du_analysis_index::Observation;
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, SizeValue};

fn file(bytes: u64) -> Option<SizeValue> {
    Some(SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    })
}

/// root/a (4 B), root/docs/, root/docs/b (6 B), and `extra` more files under root/many.
fn scope(extra: usize) -> Result<(Engine, Arc<JobRuntime>, Id), Box<dyn std::error::Error>> {
    let store = IngestSink::default();
    let mut scan = store.open(Ids.next_id()).map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    scan.add_file(Observation {
        path: vec![b"a".to_vec()],
        size: file(4),
    })
    .map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![b"docs".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    scan.add_file(Observation {
        path: vec![b"docs".to_vec(), b"b".to_vec()],
        size: file(6),
    })
    .map_err(|e| format!("{e:?}"))?;
    if extra > 0 {
        scan.add_directory(vec![b"many".to_vec()])
            .map_err(|e| format!("{e:?}"))?;
    }
    for index in 0..extra {
        scan.add_file(Observation {
            path: vec![b"many".to_vec(), format!("f{index:06}").into_bytes()],
            size: file(1),
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    let version = Ids.next_id();
    scan.seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let (engine, runtime) = register_fixture(Arc::new(ResultSets::new(store)))?;
    Ok((engine, runtime, version))
}

fn search_region(engine: &Engine) -> Value {
    let result = call(
        engine,
        "query.region.open",
        json!({"kind":"search"}),
        Channel::Gui,
    );
    assert!(result.error.is_none(), "{:?}", result.error);
    result.body["regionRef"].clone()
}

const FILES: &str = r#"{"kind":"predicate","predicate":{"kind":"type","value":"file"}}"#;

fn reason(result: ResultEnvelope) -> String {
    result
        .error
        .map(|error| error.reason_key)
        .unwrap_or_default()
}

#[test]
fn subtree_rows_come_from_every_depth_in_sort_order() -> Result<(), Box<dyn std::error::Error>> {
    let (engine, _, version) = scope(0)?;
    let region = search_region(&engine);
    let condition: Value = serde_json::from_str(FILES)?;
    let gui = |body| call(&engine, "query.run", body, Channel::Gui);
    let all = gui(
        json!({"regionRef": region, "requestSeq": 1, "resultVersion": version,
        "condition": condition, "pageSize": 1,
        "sortKeys": [{"id":"sort.size","revision":1,"descending":true}]}),
    );
    assert!(all.error.is_none(), "{:?}", all.error);
    assert_eq!(all.body["kind"], "opened");
    assert_eq!(all.body["scope"], Value::Null);
    assert_eq!(all.body["page"]["rowKind"], "search");
    assert_eq!(
        all.body["page"]["total"],
        json!({"state":"exact","count":2})
    );
    assert_eq!(all.body["page"]["rows"][0]["item"]["itemNo"], 3);
    let cursor = all.body["page"]["nextCursor"].as_str().ok_or("no cursor")?;
    let next = call(
        &engine,
        "query.page",
        json!({"nextCursor": cursor}),
        Channel::Gui,
    );
    assert!(next.error.is_none(), "{:?}", next.error);
    assert_eq!(next.body["rows"][0]["item"]["itemNo"], 1);
    assert_eq!(next.body["nextCursor"], Value::Null);

    let docs = json!({"resultVersion": version, "itemNo": 2});
    let below = gui(json!({"regionRef": region, "requestSeq": 2, "scope": docs,
        "condition": condition}));
    assert!(below.error.is_none(), "{:?}", below.error);
    assert_eq!(below.body["scope"], docs);
    assert_eq!(below.body["page"]["rows"].as_array().map(Vec::len), Some(1));
    assert_eq!(below.body["page"]["rows"][0]["item"]["itemNo"], 3);
    Ok(())
}

#[test]
fn refusals_use_the_published_reason_keys() -> Result<(), Box<dyn std::error::Error>> {
    let (engine, _, version) = scope(0)?;
    let region = search_region(&engine);
    let condition: Value = serde_json::from_str(FILES)?;
    let run = |seq: u32, extra: Value, channel| {
        let mut body = json!({"regionRef": region, "requestSeq": seq, "condition": condition});
        if let (Some(body), Value::Object(extra)) = (body.as_object_mut(), extra) {
            body.extend(extra);
        }
        reason(call(&engine, "query.run", body, channel))
    };
    assert_eq!(run(1, json!({}), Channel::Cli), "command.not_provided");
    assert_eq!(run(1, json!({}), Channel::Mcp), "command.not_provided");
    assert_eq!(
        run(1, json!({"pageSize": 0}), Channel::Gui),
        "cursor.limit_exceeded"
    );
    assert_eq!(
        run(1, json!({"pageSize": 1001}), Channel::Gui),
        "cursor.limit_exceeded"
    );
    let a_file = json!({"resultVersion": version, "itemNo": 1});
    assert_eq!(
        run(1, json!({"scope": a_file}), Channel::Gui),
        "ref.unavailable"
    );
    let missing = json!({"condition": {"kind":"predicate","predicate":{"kind":"registered",
        "id":"pred.missing","revision":1,"operand":"YQ"}}});
    assert_eq!(run(1, missing, Channel::Gui), "predicate.unknown");
    let unknown_sort = json!({"sortKeys": [{"id":"sort.missing","revision":1,"descending":false}]});
    assert_eq!(run(1, unknown_sort, Channel::Gui), "sort.key_unknown");
    assert_eq!(
        run(1, json!({"resultVersion": Ids.next_id()}), Channel::Gui),
        "result.requery_required"
    );
    assert_eq!(run(5, json!({}), Channel::Gui), "");
    assert_eq!(run(5, json!({}), Channel::Gui), "command.body_invalid");
    assert_eq!(run(4, json!({}), Channel::Gui), "command.body_invalid");
    let browse = reason(call(
        &engine,
        "query.run",
        json!({"regionRef": super::region(&engine), "requestSeq": 9, "condition": condition}),
        Channel::Gui,
    ));
    assert_eq!(browse, "ref.unavailable");
    Ok(())
}

#[test]
fn a_large_version_returns_a_job_whose_reference_reads_the_search()
-> Result<(), Box<dyn std::error::Error>> {
    let (engine, runtime, version) = scope(super::super::super::run::SYNC_ITEMS)?;
    let region = search_region(&engine);
    let condition: Value = serde_json::from_str(FILES)?;
    let docs = json!({"resultVersion": version, "itemNo": 2});
    let job = call(
        &engine,
        "query.run",
        json!({"regionRef": region, "requestSeq": 1, "scope": docs, "condition": condition}),
        Channel::Gui,
    );
    assert!(job.error.is_none(), "{:?}", job.error);
    assert_eq!(job.body["kind"], "job");
    assert_eq!(job.body["resultVersion"], json!(version));
    let job_ref: Id = serde_json::from_value(job.body["jobRef"].clone())?;
    let query = await_query(&runtime, &job_ref)?;
    let page = call(
        &engine,
        "query.page",
        json!({"queryRef": query, "resultVersion": version, "start": 0, "count": 10}),
        Channel::Gui,
    );
    assert!(page.error.is_none(), "{:?}", page.error);
    assert_eq!(page.body["rowKind"], "search");
    assert_eq!(page.body["rows"].as_array().map(Vec::len), Some(1));
    // docs holds one file; folders below the scope are candidates but not files.
    assert_eq!(page.body["rows"][0]["kind"], "file");
    Ok(())
}
