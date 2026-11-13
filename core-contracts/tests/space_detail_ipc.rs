use du_core_contracts::space_detail_ipc::SpaceDetail;
use serde_json::json;

#[test]
fn preserves_zero_unknown_and_uncollected_detail_evidence() -> Result<(), serde_json::Error> {
    let version = "018f0000-0000-7000-8000-000000000001";
    let missing = json!({"state":"unknown","reasonKey":"space.data_insufficient"});
    let wire = json!({
        "item":{"resultVersion":version,"itemNo":3},
        "logical":{"state":"present","value":{
            "size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"0"},
            "measurementLimits":[]
        }},
        "allocated":missing,
        "uniquePhysical":missing,
        "exclusive":missing,
        "shared":missing,
        "linkGroup":{"state":"present","value":{
            "reportedLinkCount":"18446744073709551615",
            "inScopePaths":[{"resultVersion":version,"itemNo":3}],
            "hasOutsideLinks":null
        }},
        "auxiliary":{"state":"unknown","reasonKey":"space.aux_not_included"},
        "exclusiveDecision":{"confirmed":false,"reasonKey":"space.data_insufficient"},
        "observedAt":missing,
        "capabilities":missing
    });
    let detail: SpaceDetail = serde_json::from_value(wire.clone())?;
    assert_eq!(serde_json::to_value(detail)?, wire);
    Ok(())
}
