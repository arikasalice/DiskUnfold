use super::*;
use serde_json::{Value, json};

fn folder_page() -> Value {
    let id = "018f0000-0000-7000-8000-000000000001";
    json!({"rowKind":"folder","queryRef":id,"resultVersion":id,"rows":[{
        "item":{"resultVersion":id,"itemNo":0}, "displayName":{"state":"unknown","reasonKey":"ref.unavailable"},
        "kind":"directory","logicalSize":{"state":"absent"},"allocatedSize":{"state":"absent"},
        "modifiedAt":{"state":"absent"},"hasChildFolder":{"state":"unknown","reasonKey":"ref.unavailable"},
        "flags":{"unenumerated":{"state":"unknown","reasonKey":"ref.unavailable"},"excluded":{"state":"absent"}},
        "correspondence":{"state":"present","value":{"volumeId":"YQ","pathCompareKey":"Yg","fileId":null,"identityCapability":{"state":"supported"}}}
    }], "total":{"state":"lower_bound","count":1},"unknownCount":0,"observedAt":null,"completeness":"partial","nextCursor":null})
}

#[test]
fn opened_and_job_are_distinct_and_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let id = "018f0000-0000-7000-8000-000000000001";
    let opened = json!({"kind":"opened","requestSeq":1,"origin":"scan","location":null,"breadcrumbs":[],"rootKind":"virtual_root","currentCorrespondence":false,"page":folder_page(),"sortStrategies":[],"predicateStrategies":[]});
    let result: BrowseOpenResponse = serde_json::from_value(opened.clone())?;
    assert_eq!(serde_json::to_value(result)?, opened);
    let mut future = opened.clone();
    future["origin"] = json!("future_origin");
    assert!(serde_json::from_value::<BrowseOpenResponse>(future).is_ok());
    let job = json!({"kind":"job","requestSeq":1,"jobRef":id});
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<BrowseOpenResponse>(job.clone())?)?,
        job
    );
    assert!(
        serde_json::from_value::<BrowseOpenResponse>(
            json!({"kind":"job","requestSeq":1,"jobRef":id,"page":folder_page()})
        )
        .is_err()
    );
    let mut missing = opened;
    missing
        .as_object_mut()
        .ok_or("not object")?
        .remove("location");
    assert!(serde_json::from_value::<BrowseOpenResponse>(missing).is_err());
    Ok(())
}

#[test]
fn page_discriminates_rows_and_requires_nullable_fields() -> Result<(), Box<dyn std::error::Error>>
{
    let page = folder_page();
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<QueryPageResponse>(page.clone())?)?,
        page
    );
    let mut bad_capability = page.clone();
    bad_capability["rows"][0]["correspondence"]["value"]["identityCapability"]["state"] =
        json!("unknown");
    assert!(serde_json::from_value::<QueryPageResponse>(bad_capability).is_err());
    let mut missing_id = page.clone();
    missing_id["rows"][0]["correspondence"]["value"]
        .as_object_mut()
        .ok_or("not object")?
        .remove("fileId");
    assert!(serde_json::from_value::<QueryPageResponse>(missing_id).is_err());
    let mut missing = page.clone();
    missing
        .as_object_mut()
        .ok_or("not object")?
        .remove("observedAt");
    assert!(serde_json::from_value::<QueryPageResponse>(missing).is_err());
    let mut mismatch = page.clone();
    mismatch["rowKind"] = json!("search");
    assert!(serde_json::from_value::<QueryPageResponse>(mismatch).is_err());
    let mut overflow = page;
    overflow["unknownCount"] = json!(u64::from(u32::MAX) + 1);
    assert!(serde_json::from_value::<QueryPageResponse>(overflow).is_err());
    let id = "018f0000-0000-7000-8000-000000000001";
    let search = json!({"rowKind":"search","queryRef":id,"resultVersion":id,"rows":[{"item":{"resultVersion":id,"itemNo":1},"displayName":{"state":"absent"},"displayPath":{"state":"absent"},"kind":"file","logicalSize":{"state":"absent"},"allocatedSize":{"state":"absent"},"modifiedAt":{"state":"absent"},"createdAt":{"state":"absent"},"missingFields":[],"reasonKeys":[]}],"total":{"state":"exact","count":1},"unknownCount":0,"observedAt":null,"completeness":"complete","nextCursor":null});
    let mut duplicate = search.clone();
    duplicate["rows"][0]["missingFields"] = json!(["size", "size"]);
    assert!(serde_json::from_value::<QueryPageResponse>(duplicate).is_err());
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<QueryPageResponse>(search.clone())?)?,
        search
    );
    Ok(())
}

#[test]
fn folder_context_round_trips_and_requires_its_nullable_location()
-> Result<(), Box<dyn std::error::Error>> {
    let mut page = folder_page();
    page["browseContext"] = json!({"origin":"scan", "location":null, "breadcrumbs":[], "rootKind":"virtual_root", "currentCorrespondence":false, "sortStrategies":[], "predicateStrategies":[]});
    let parsed: QueryPageResponse = serde_json::from_value(page.clone())?;
    assert_eq!(serde_json::to_value(parsed)?, page);
    page["browseContext"]
        .as_object_mut()
        .ok_or("not an object")?
        .remove("location");
    assert!(serde_json::from_value::<QueryPageResponse>(page).is_err());
    Ok(())
}
