use super::ranked_bar_runtime::{fixture, request};
use super::*;

#[test]
fn group_rows_dispatch_preserves_evidence_and_maps_public_errors() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 75])?;
    let fixture = fixture(sink.clone())?;
    let view = fixture.open()?;
    let mut first_request = request(&view, 1);
    first_request["viewport"]["width"] = json!(4);
    first_request["viewport"]["height"] = json!(4);
    let physical = job(&fixture.ok("viz.render", first_request.clone())?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|error| error.to_string())?;
    fixture
        .release
        .send(())
        .map_err(|error| error.to_string())?;
    fixture.finished(&physical, JobState::Done)?;
    let mut second_request = request(&view, 2);
    second_request["viewport"]["width"] = json!(4);
    second_request["viewport"]["height"] = json!(4);
    let rendered = fixture.ok("viz.render", second_request)?;
    let model_ref = rendered["model"]["modelRef"]
        .as_str()
        .ok_or("model reference missing")?;
    let group_request = |count| {
        json!({
            "modelRef": model_ref,
            "groupRef": "fixture.other",
            "start": 0,
            "count": count
        })
    };

    let one = fixture.call("viz.group_rows", group_request(1));
    assert!(one.error.is_none());
    assert_eq!(one.body["rows"].as_array().ok_or("rows")?.len(), 1);
    assert_eq!(one.body["totalCount"], 2);
    let full = fixture.call("viz.group_rows", group_request(1000));
    assert!(full.error.is_none());
    assert_eq!(full.body["rows"].as_array().ok_or("rows")?.len(), 2);
    assert_eq!(full.body["resultRevision"], json!(id(3)));
    assert_eq!(full.body["groupTotal"]["bytes"], "100");
    assert_eq!(full.body["sizeRange"]["kind"], "unknown");
    assert_eq!(full.body["observedAt"], rendered["observedAt"]);
    assert_eq!(full.body["completeness"], rendered["completeness"]);
    let members = full.body["rows"].as_array().ok_or("rows")?;
    assert!(
        members
            .iter()
            .all(|row| row["itemRef"]["resultVersion"] == json!(id(3)))
    );
    assert_ne!(members[0]["itemRef"], members[1]["itemRef"]);

    check_invalid_pages(&fixture, model_ref)?;

    publish_sizes(&sink, id(2), id(4), [25, 75])?;
    let stale = fixture.call("viz.group_rows", group_request(1));
    let stale_error = stale.error.ok_or("stale model accepted")?;
    assert_eq!(stale_error.code, du_core_contracts::ErrorCode::StaleVersion);
    assert_eq!(stale_error.reason_key, "result.version_stale");
    Ok(())
}

fn check_invalid_pages(fixture: &Fixture, model_ref: &str) -> Result<(), String> {
    let group_request =
        |count| json!({"modelRef":model_ref,"groupRef":"fixture.other","start":0,"count":count});
    let zero = fixture.call("viz.group_rows", group_request(0));
    let zero_error = zero.error.ok_or("zero count accepted")?;
    assert_eq!(
        zero_error.code,
        du_core_contracts::ErrorCode::InvalidArgument
    );
    let oversized = fixture.call("viz.group_rows", group_request(1001));
    let oversized_error = oversized.error.ok_or("oversized page accepted")?;
    assert_eq!(
        oversized_error.code,
        du_core_contracts::ErrorCode::InvalidArgument
    );

    let mismatch = fixture.call(
        "viz.group_rows",
        json!({"modelRef":model_ref,"groupRef":"missing","start":0,"count":1}),
    );
    let mismatch_error = mismatch.error.ok_or("unowned group accepted")?;
    assert_eq!(
        mismatch_error.code,
        du_core_contracts::ErrorCode::InvalidArgument
    );
    assert_eq!(mismatch_error.reason_key, "command.body_invalid");

    let expired = fixture.call(
        "viz.group_rows",
        json!({"modelRef":"expired","groupRef":"fixture.other","start":0,"count":1}),
    );
    let expired_error = expired.error.ok_or("expired model accepted")?;
    assert_eq!(expired_error.code, du_core_contracts::ErrorCode::NotFound);
    assert_eq!(expired_error.reason_key, "viz.model_expired");

    Ok(())
}
