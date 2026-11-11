use super::*;

#[test]
fn omitted_revision_selects_current_scan_but_explicit_revision_cannot_drift() -> Result<(), String>
{
    let fixture = Fixture::new([25, 75])?;
    let view = fixture.open()?;
    publish(&fixture.sink, id(2), id(4))?;
    let mut request = render_body(&view, 1);
    request["sourceRef"] = json!({"kind": "scan", "ref": id(1)});
    assert_eq!(
        fixture
            .call("viz.render", request.clone())
            .error
            .ok_or("stale explicit result was substituted")?
            .reason_key,
        "result.version_stale"
    );
    request["sourceRef"]["ref"] = json!(id(9));
    assert_eq!(
        fixture
            .call("viz.render", request.clone())
            .error
            .ok_or("unknown lineage was substituted")?
            .reason_key,
        "result.version_not_found"
    );
    request["sourceRef"]["ref"] = json!(id(1));
    request
        .as_object_mut()
        .ok_or("invalid fixture request")?
        .remove("resultRevision");
    let root = json!({"resultVersion": id(4), "itemNo": 0});
    request["aggregateScopeRef"] = root.clone();
    request["navigationScopeRef"] = root;
    let response = fixture.ok("viz.render", request)?;
    assert_eq!(response["resultRevision"], json!(id(4)));
    let job_ref = job(&response)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|error| error.to_string())?;
    fixture
        .release
        .send(())
        .map_err(|error| error.to_string())?;
    fixture.finished(&job_ref, JobState::Done)
}
