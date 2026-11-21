use super::*;
use crate::api::Engine;
use du_analysis_index::{FileIdentity, Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ErrorCode, Id, PathBytes, PathEncoding, ResultEnvelope,
    SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use serde_json::{Value, json};

fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}

fn debug(value: impl std::fmt::Debug) -> String {
    format!("{value:?}")
}

fn root(name: &[u8]) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(name),
        },
        volume_id: vec![1],
        file_id: name.to_vec(),
    }
}

fn size(basis: SizeBasis, evidence: SizeEvidence, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}

/// Two roots: /a holds v.mp4 and a.pdf, /b holds b.pdf, which is the same file as a.pdf.
fn two_roots() -> Result<(IngestSink, Id), String> {
    let store = IngestSink::default();
    let mut build = store
        .open_forest(id(1), vec![root(b"/a"), root(b"/b")])
        .map_err(debug)?;
    build.add_directory_at(0, Vec::new()).map_err(debug)?;
    build.add_directory_at(1, Vec::new()).map_err(debug)?;
    for (root, name, file) in [(0, "v.mp4", 8_u8), (0, "a.pdf", 7), (1, "b.pdf", 7)] {
        build
            .add_with_space_at(
                root,
                Observation {
                    path: vec![name.as_bytes().to_vec()],
                    size: Some(size(SizeBasis::Logical, SizeEvidence::Measured, 100)),
                },
                SpaceObservation {
                    allocated: Some(size(SizeBasis::Allocated, SizeEvidence::OsReported, 4096)),
                    identity: Some(FileIdentity {
                        volume: vec![1],
                        file: vec![file],
                    }),
                    link_count: Some(if file == 7 { 2 } else { 1 }),
                    sharing: SharingEvidence::None,
                },
            )
            .map_err(debug)?;
    }
    build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(debug)?;
    Ok((store, id(2)))
}

fn engine(store: IngestSink) -> Result<Engine, String> {
    let mut builder = EngineBuilder::default();
    register(&mut builder, store);
    builder.build().map_err(debug)
}

fn ask(engine: &Engine, channel: Channel, body: Value) -> Result<ResultEnvelope, String> {
    let correlation = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let caller = CallerCtx::new(channel, correlation.clone(), None);
    Ok(engine.call(
        CallEnvelope {
            name: "space.type_summary".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: correlation,
            body,
        },
        &caller,
    ))
}

fn gui(engine: &Engine, body: Value) -> Result<ResultEnvelope, String> {
    ask(engine, Channel::Gui, body)
}

fn reason(response: ResultEnvelope, code: ErrorCode, reason: &str) -> Result<(), String> {
    let error = response.error.ok_or("request accepted")?;
    assert_eq!(error.code, code);
    assert_eq!(error.reason_key, reason);
    Ok(())
}

#[test]
fn every_root_is_counted_in_one_walk_in_legend_order_with_a_shared_file_once() -> Result<(), String>
{
    let (store, version) = two_roots()?;
    let engine = engine(store)?;
    let allocated = gui(
        &engine,
        json!({"resultVersion": version, "basis": "allocated"}),
    )?;
    assert!(allocated.error.is_none());
    let slices = allocated.body["slices"].as_array().ok_or("no slices")?;
    let rows: Vec<_> = slices
        .iter()
        .map(|slice| {
            (
                slice["classKey"].as_str().unwrap_or_default().to_owned(),
                slice["itemCount"].to_string(),
                slice["total"].to_string(),
            )
        })
        .collect();
    // Both classes hold 4096 bytes, so the tie is in legend order. The shared pdf is
    // two items and one allocation.
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "type.document");
    assert_eq!(rows[1].0, "type.video");
    assert!(rows[0].1.contains('2') && rows[0].2.contains("4096"));
    assert!(rows[1].1.contains('1') && rows[1].2.contains("4096"));
    assert_eq!(allocated.body["lowerBound"], false);
    let order = allocated.body["legendOrder"].as_array().ok_or("no order")?;
    assert_eq!(order.len(), 9);
    assert_eq!(order[0], "type.document");
    let logical = gui(
        &engine,
        json!({"resultVersion": version, "basis": "logical"}),
    )?;
    assert!(
        logical.body["slices"][0]["total"]
            .to_string()
            .contains("200")
    );
    Ok(())
}

#[test]
fn an_empty_list_means_every_range_was_read_and_there_is_no_file() -> Result<(), String> {
    let store = IngestSink::default();
    let mut build = store.open_forest(id(1), vec![root(b"/a")]).map_err(debug)?;
    build.add_directory_at(0, Vec::new()).map_err(debug)?;
    build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(debug)?;
    let engine = engine(store)?;
    let answer = gui(
        &engine,
        json!({"resultVersion": id(2), "basis": "allocated"}),
    )?;
    assert!(answer.error.is_none());
    assert_eq!(answer.body["slices"], json!([]));
    assert_eq!(answer.body["lowerBound"], false);
    Ok(())
}

#[test]
fn an_unread_range_makes_the_answer_a_lower_bound_even_when_nothing_was_counted()
-> Result<(), String> {
    let store = IngestSink::default();
    let mut build = store.open_forest(id(1), vec![root(b"/a")]).map_err(debug)?;
    build.add_directory_at(0, Vec::new()).map_err(debug)?;
    build
        .add_directory_at(0, vec![b"locked".to_vec()])
        .map_err(debug)?;
    build
        .record_gap_at(
            0,
            vec![b"locked".to_vec()],
            "scan.unclassified".into(),
            None,
        )
        .map_err(debug)?;
    build
        .seal(id(2), ResultCompleteness::Missing)
        .map_err(debug)?;
    let engine = engine(store)?;
    let answer = gui(
        &engine,
        json!({"resultVersion": id(2), "basis": "allocated"}),
    )?;
    assert!(answer.error.is_none());
    assert_eq!(answer.body["slices"], json!([]));
    assert_eq!(answer.body["lowerBound"], true);
    Ok(())
}

#[test]
fn only_the_gui_asks_and_the_request_has_two_known_fields() -> Result<(), String> {
    let (store, version) = two_roots()?;
    let engine = engine(store)?;
    let body = json!({"resultVersion": version, "basis": "logical"});
    for channel in [Channel::Cli, Channel::Mcp] {
        let refused = ask(&engine, channel, body.clone())?;
        assert!(refused.error.is_some());
        assert!(refused.body.is_null());
    }
    for bad in [
        json!({"resultVersion": version}),
        json!({"resultVersion": version, "basis": null}),
        json!({"resultVersion": version, "basis": "logical", "extra": 1}),
        json!({"resultVersion": null, "basis": "logical"}),
    ] {
        let refused = gui(&engine, bad)?;
        assert!(refused.error.is_some());
        assert!(refused.body.is_null());
    }
    Ok(())
}

#[test]
fn another_version_is_stale_against_the_current_one() -> Result<(), String> {
    let (store, version) = two_roots()?;
    let mut replacement = store.open(id(3)).map_err(debug)?;
    replacement.add_directory(Vec::new()).map_err(debug)?;
    replacement
        .seal(id(4), ResultCompleteness::Complete)
        .map_err(debug)?;
    let engine = engine(store)?;
    reason(
        gui(
            &engine,
            json!({"resultVersion": version, "basis": "logical"}),
        )?,
        ErrorCode::StaleVersion,
        "result.requery_required",
    )?;
    // A version the store never held is stale against the one it holds.
    reason(
        gui(&engine, json!({"resultVersion": id(9), "basis": "logical"}))?,
        ErrorCode::StaleVersion,
        "result.requery_required",
    )
}

#[test]
fn a_sample_result_has_no_classes() -> Result<(), String> {
    use du_analysis_index::SampleSessionKey;
    let store = IngestSink::default();
    let mut sample = store
        .open_sample_forest(id(3), vec![root(b"/s")], &SampleSessionKey::new(id(9)))
        .map_err(debug)?;
    sample.add_directory_at(0, Vec::new()).map_err(debug)?;
    sample
        .seal(id(4), ResultCompleteness::Complete)
        .map_err(debug)?;
    let engine = engine(store)?;
    let answer = gui(
        &engine,
        json!({"resultVersion": id(4), "basis": "allocated"}),
    )?;
    assert!(answer.error.is_some(), "{:?}", answer.body);
    assert!(answer.body.is_null());
    Ok(())
}
