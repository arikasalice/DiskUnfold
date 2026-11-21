use super::*;
use serde_json::json;

type Outcome = Result<(), Box<dyn std::error::Error>>;

fn id() -> Result<Id, serde_json::Error> {
    serde_json::from_value(json!("01890f7e-8b5a-7cc0-8000-000000000001"))
}

#[test]
fn request_accepts_the_minimum_and_rejects_unknown_fields() -> Outcome {
    let id = id()?;
    let minimum = json!({
        "regionRef": id, "requestSeq": 1,
        "condition": {"kind": "all", "children": []}
    });
    let request: QueryRunRequest = serde_json::from_value(minimum.clone())?;
    assert_eq!(request.scope, None);
    assert_eq!(serde_json::to_value(&request)?, minimum);

    let mut extra = minimum.clone();
    extra["location"] = json!({"resultVersion": id, "itemNo": 1});
    assert!(serde_json::from_value::<QueryRunRequest>(extra).is_err());
    let mut missing = minimum;
    missing
        .as_object_mut()
        .ok_or("not an object")?
        .remove("condition");
    assert!(serde_json::from_value::<QueryRunRequest>(missing).is_err());
    Ok(())
}

#[test]
fn opened_requires_the_scope_field_and_job_round_trips() -> Outcome {
    let id = id()?;
    let page = json!({"rowKind":"search","queryRef":id,"resultVersion":id,"rows":[],
        "total":{"state":"exact","count":0},"unknownCount":0,"observedAt":null,
        "completeness":"complete","nextCursor":null});
    let opened = json!({"kind":"opened","requestSeq":2,"resultVersion":id,"origin":"scan",
        "scope":null,"page":page});
    let value: QueryRunResponse = serde_json::from_value(opened.clone())?;
    assert_eq!(serde_json::to_value(&value)?, opened);
    let mut absent = opened;
    absent
        .as_object_mut()
        .ok_or("not an object")?
        .remove("scope");
    assert!(serde_json::from_value::<QueryRunResponse>(absent).is_err());

    let job = json!({"kind":"job","requestSeq":3,"jobRef":id,"resultVersion":id});
    let value: QueryRunResponse = serde_json::from_value(job.clone())?;
    assert_eq!(serde_json::to_value(&value)?, job);
    Ok(())
}
