use super::*;
use du_core_contracts::browse_page_ipc::{BrowseOpenResponse, QueryPageResponse};

/// Request bodies keep strict validation; responses tolerate fields from a later revision.
#[test]
fn browse_requests_reject_and_responses_accept_unknown_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let (engine, _, version) = fixture(2)?;
    let region_ref = region(&engine);
    let request = json!({"regionRef":region_ref,"viewKind":"list","requestSeq":1,"resultVersion":version,"pageSize":1});
    let mut extended = Vec::new();
    let mut top = request.clone();
    top["futureField"] = json!(true);
    extended.push(top);
    let mut sort = request.clone();
    sort["sortKeys"] =
        json!([{"id":"sort.path","revision":1,"descending":false,"futureField":true}]);
    extended.push(sort);
    let mut condition = request.clone();
    condition["condition"] = json!({"kind":"predicate","predicate":{"kind":"type","value":"directory","futureField":true}});
    extended.push(condition);
    for body in extended {
        let rejected = call(&engine, "browse.open", body, Channel::Gui);
        assert_eq!(
            rejected
                .error
                .ok_or("accepted an unknown field")?
                .reason_key,
            "command.body_invalid"
        );
    }

    // The rejected bodies admitted nothing, so the same sequence still opens.
    let opened = call(&engine, "browse.open", request, Channel::Gui);
    assert!(opened.error.is_none(), "{:?}", opened.error);
    assert_eq!(opened.body["kind"], "opened");
    let mut response = opened.body.clone();
    response["futureField"] = json!(true);
    response["page"]["futureField"] = json!(true);
    response["page"]["rows"][0]["futureField"] = json!(true);
    response["page"]["rows"][0]["flags"]["futureField"] = json!(true);
    response["page"]["total"]["futureField"] = json!(true);
    let decoded: BrowseOpenResponse = serde_json::from_value(response)?;
    assert_eq!(serde_json::to_value(decoded)?, opened.body);

    let cursor = opened.body["page"]["nextCursor"].clone();
    let next = call(
        &engine,
        "query.page",
        json!({"nextCursor":cursor,"count":1}),
        Channel::Gui,
    );
    assert!(next.error.is_none(), "{:?}", next.error);
    let mut page = next.body.clone();
    page["rows"][0]["futureField"] = json!(true);
    let decoded: QueryPageResponse = serde_json::from_value(page)?;
    assert_eq!(serde_json::to_value(decoded)?, next.body);
    Ok(())
}
