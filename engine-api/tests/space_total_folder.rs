use du_analysis_index::IngestSink;
use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION, ErrorCode, Id};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder},
    space,
};
use serde_json::json;

/// A conflicting folder reference is stale even before any results have been published.
#[test]
fn folder_total_rejects_conflicting_revisions_before_lookup()
-> Result<(), Box<dyn std::error::Error>> {
    let mut builder = EngineBuilder::default();
    space::register_total(&mut builder, IngestSink::default());
    let engine = builder.build()?;
    let correlation = Id::parse("018f0000-0000-7000-8000-000000000001")?;
    let requested = Id::parse("018f0000-0000-7000-8000-000000000002")?;
    let conflicting = Id::parse("018f0000-0000-7000-8000-000000000003")?;
    let caller = CallerCtx::new(Channel::Gui, correlation.clone(), None);
    let call = |item_version: &Id| {
        engine.call(
            CallEnvelope {
                name: "space.total".into(),
                contract_version: ENVELOPE_VERSION,
                correlation_id: correlation.clone(),
                body: json!({
                    "requestSeq": 1,
                    "resultVersion": requested,
                    "target": {"kind": "folder", "item": {"resultVersion": item_version, "itemNo": 0}}
                }),
            },
            &caller,
        )
    };
    let mismatch = call(&conflicting)
        .error
        .ok_or("conflicting revisions accepted")?;
    assert_eq!(mismatch.code, ErrorCode::StaleVersion);
    assert_eq!(mismatch.reason_key, "result.requery_required");
    let missing = call(&requested).error.ok_or("missing result accepted")?;
    assert_eq!(missing.code, ErrorCode::NotFound);
    assert_eq!(missing.reason_key, "ref.unavailable");
    Ok(())
}
