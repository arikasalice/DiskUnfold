//! Pins the optional scan-status facts of browse responses: the folder boundary of a
//! row and the excluded child count of the opened folder. A response without them
//! still decodes, and unknown fields and boundary names are accepted.
use du_core_contracts::{
    ErrorEnvelope, Id,
    browse_ipc::BrowseValue,
    browse_page_ipc::{
        BrowseOpenResponse, FolderBoundary, FolderFlags, FolderQueryPage, QueryPageResponse,
        QueryTotal,
    },
};
use serde_json::{Value, from_value, json, to_value};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const ID: &str = "018f0000-0000-7000-8000-000000000001";

fn unknown(reason: &str) -> Value {
    json!({ "state": "unknown", "reasonKey": reason })
}

/// A folder page as it was sent before these fields existed.
fn earlier_page() -> Value {
    json!({
        "rowKind": "folder", "queryRef": ID, "resultVersion": ID,
        "rows": [{
            "item": { "resultVersion": ID, "itemNo": 1 },
            "displayName": { "state": "present", "value": "Photos" },
            "kind": "directory",
            "logicalSize": { "state": "absent" },
            "allocatedSize": { "state": "absent" },
            "modifiedAt": { "state": "absent" },
            "hasChildFolder": unknown("ref.unavailable"),
            "flags": {
                "unenumerated": unknown("ref.unavailable"),
                "excluded": unknown("ref.unavailable"),
            },
            "correspondence": unknown("ref.unavailable"),
        }],
        "total": { "state": "exact", "count": 1 }, "unknownCount": 0,
        "observedAt": null, "completeness": "partial", "nextCursor": null,
    })
}

/// The location context of a folder page as it was sent before these fields existed.
fn earlier_context() -> Value {
    json!({
        "origin": "scan", "location": { "resultVersion": ID, "itemNo": 0 },
        "breadcrumbs": [{
            "item": { "resultVersion": ID, "itemNo": 0 },
            "displayName": { "state": "present", "value": "C:\\Data" },
        }],
        "rootKind": "scope_root", "currentCorrespondence": false,
        "sortStrategies": [], "predicateStrategies": [],
    })
}

/// An opened response as it was sent before these fields existed.
fn earlier_opened() -> Value {
    let mut opened = earlier_context();
    opened["kind"] = json!("opened");
    opened["requestSeq"] = json!(1);
    opened["page"] = earlier_page();
    opened
}

fn folder(page: QueryPageResponse) -> Result<FolderQueryPage, &'static str> {
    match page {
        QueryPageResponse::Folder(page) => Ok(page),
        QueryPageResponse::Search(_) => Err("expected a folder page"),
    }
}

fn boundary_of(
    page: &Value,
) -> Result<Option<BrowseValue<FolderBoundary>>, Box<dyn std::error::Error>> {
    let page = folder(from_value(page.clone())?)?;
    let row = page.page.rows.first().ok_or("missing row")?;
    Ok(row.flags.boundary.clone())
}

fn present(name: &str) -> BrowseValue<FolderBoundary> {
    BrowseValue::Present {
        value: FolderBoundary::new(name),
    }
}

#[test]
fn earlier_responses_without_the_new_fields_still_decode() -> TestResult {
    let page = earlier_page();
    assert_eq!(boundary_of(&page)?, None);
    let decoded: QueryPageResponse = from_value(page.clone())?;
    assert_eq!(to_value(decoded)?, page);

    let mut with_context = earlier_page();
    with_context["browseContext"] = earlier_context();
    let decoded = folder(from_value(with_context.clone())?)?;
    let context = decoded.browse_context.as_ref().ok_or("missing context")?;
    assert_eq!(context.excluded_children, None);
    assert_eq!(to_value(QueryPageResponse::Folder(decoded))?, with_context);

    let opened = earlier_opened();
    let decoded: BrowseOpenResponse = from_value(opened.clone())?;
    let BrowseOpenResponse::Opened {
        excluded_children,
        page,
        ..
    } = &decoded
    else {
        return Err("expected an opened response".into());
    };
    assert_eq!(*excluded_children, None);
    assert_eq!(page.page.rows[0].flags.boundary, None);
    assert_eq!(to_value(decoded)?, opened);
    Ok(())
}

#[test]
fn boundary_names_are_the_published_strings() {
    assert_eq!(FolderBoundary::OTHER_VOLUME, "other_volume");
    assert_eq!(FolderBoundary::ALREADY_COUNTED, "already_counted");
    assert_eq!(
        FolderBoundary::new(FolderBoundary::OTHER_VOLUME).as_str(),
        "other_volume"
    );
}

#[test]
fn a_null_field_reads_like_an_omitted_one() -> TestResult {
    let mut page = earlier_page();
    page["rows"][0]["flags"]["boundary"] = Value::Null;
    assert_eq!(boundary_of(&page)?, None);

    let mut opened = earlier_opened();
    opened["excludedChildren"] = Value::Null;
    let BrowseOpenResponse::Opened {
        excluded_children, ..
    } = from_value(opened)?
    else {
        return Err("expected an opened response".into());
    };
    assert_eq!(excluded_children, None);

    let mut with_context = earlier_page();
    with_context["browseContext"] = earlier_context();
    with_context["browseContext"]["excludedChildren"] = Value::Null;
    let decoded = folder(from_value(with_context)?)?;
    let context = decoded.browse_context.as_ref().ok_or("missing context")?;
    assert_eq!(context.excluded_children, None);
    Ok(())
}

#[test]
fn a_row_boundary_round_trips_in_every_state() -> TestResult {
    let cases = [
        (
            json!({ "state": "present", "value": "other_volume" }),
            present(FolderBoundary::OTHER_VOLUME),
        ),
        (
            json!({ "state": "present", "value": "already_counted" }),
            present(FolderBoundary::ALREADY_COUNTED),
        ),
        // A name added by a later contract version is kept as it was sent.
        (
            json!({ "state": "present", "value": "future_rule" }),
            present("future_rule"),
        ),
        (json!({ "state": "absent" }), BrowseValue::Absent),
        (
            unknown("scan.in_progress"),
            BrowseValue::Unknown {
                reason_key: "scan.in_progress".into(),
            },
        ),
        (
            unknown("space.data_insufficient"),
            BrowseValue::Unknown {
                reason_key: "space.data_insufficient".into(),
            },
        ),
    ];
    for (wire, expected) in cases {
        let mut page = earlier_page();
        page["rows"][0]["flags"]["boundary"] = wire.clone();
        assert_eq!(boundary_of(&page)?, Some(expected.clone()), "{wire}");
        let decoded: QueryPageResponse = from_value(page.clone())?;
        assert_eq!(to_value(decoded)?, page, "{wire}");

        let mut opened = earlier_opened();
        opened["page"] = page;
        let decoded: BrowseOpenResponse = from_value(opened.clone())?;
        assert_eq!(to_value(decoded)?, opened, "{wire}");

        // The serialized flags carry the fact under the name `boundary`.
        let flags = FolderFlags {
            unenumerated: BrowseValue::Present { value: false },
            excluded: BrowseValue::Present { value: false },
            boundary: Some(expected),
        };
        let absent = json!({ "state": "present", "value": false });
        assert_eq!(
            to_value(flags)?,
            json!({ "unenumerated": absent, "excluded": absent, "boundary": wire })
        );
    }
    Ok(())
}

#[test]
fn an_omitted_boundary_is_not_serialized() -> TestResult {
    let flags = FolderFlags {
        unenumerated: BrowseValue::Present { value: true },
        excluded: BrowseValue::Absent,
        boundary: None,
    };
    assert_eq!(
        to_value(flags)?,
        json!({
            "unenumerated": { "state": "present", "value": true },
            "excluded": { "state": "absent" },
        })
    );
    // A boundary is a tagged value; a bare name or a value without a state is refused.
    for invalid in [
        json!("other_volume"),
        json!({ "value": "other_volume" }),
        json!({ "state": "present" }),
    ] {
        let mut page = earlier_page();
        page["rows"][0]["flags"]["boundary"] = invalid.clone();
        assert!(from_value::<QueryPageResponse>(page).is_err(), "{invalid}");
    }
    Ok(())
}

#[test]
fn the_excluded_child_count_round_trips_on_opened_and_on_the_page_context() -> TestResult {
    let cases = [
        (
            json!({ "state": "exact", "count": 0 }),
            QueryTotal::Exact { count: 0 },
        ),
        (
            json!({ "state": "exact", "count": 12 }),
            QueryTotal::Exact { count: 12 },
        ),
        (
            json!({ "state": "lower_bound", "count": 3 }),
            QueryTotal::LowerBound { count: 3 },
        ),
    ];
    for (wire, expected) in cases {
        let mut opened = earlier_opened();
        opened["excludedChildren"] = wire.clone();
        let decoded: BrowseOpenResponse = from_value(opened.clone())?;
        let BrowseOpenResponse::Opened {
            excluded_children, ..
        } = &decoded
        else {
            return Err("expected an opened response".into());
        };
        assert_eq!(excluded_children.as_ref(), Some(&expected), "{wire}");
        assert_eq!(to_value(decoded)?, opened, "{wire}");

        let mut page = earlier_page();
        page["browseContext"] = earlier_context();
        page["browseContext"]["excludedChildren"] = wire.clone();
        let decoded = folder(from_value(page.clone())?)?;
        let context = decoded.browse_context.as_ref().ok_or("missing context")?;
        assert_eq!(
            context.excluded_children.as_ref(),
            Some(&expected),
            "{wire}"
        );
        assert_eq!(
            to_value(QueryPageResponse::Folder(decoded))?,
            page,
            "{wire}"
        );
    }
    // The count is the existing query total: a state and an unsigned 32-bit count.
    for invalid in [
        json!(3),
        json!({ "count": 3 }),
        json!({ "state": "exact" }),
        json!({ "state": "exact", "count": -1 }),
        json!({ "state": "exact", "count": u64::from(u32::MAX) + 1 }),
    ] {
        let mut opened = earlier_opened();
        opened["excludedChildren"] = invalid.clone();
        assert!(
            from_value::<BrowseOpenResponse>(opened).is_err(),
            "{invalid}"
        );
    }
    Ok(())
}

#[test]
fn the_new_values_accept_unknown_fields() -> TestResult {
    let added = json!({ "addedLater": true });
    let mut page = earlier_page();
    page["rows"][0]["flags"]["boundary"] = json!({ "state": "present", "value": "other_volume" });
    page["browseContext"] = earlier_context();
    page["browseContext"]["excludedChildren"] = json!({ "state": "lower_bound", "count": 2 });
    let mut extended = page.clone();
    extended["rows"][0]["flags"]["futureField"] = added.clone();
    extended["rows"][0]["flags"]["boundary"]["futureField"] = added.clone();
    extended["browseContext"]["futureField"] = added.clone();
    extended["browseContext"]["excludedChildren"]["futureField"] = added.clone();
    let decoded: QueryPageResponse = from_value(extended)?;
    assert_eq!(to_value(decoded)?, page);

    let mut opened = earlier_opened();
    let mut inner = page;
    inner
        .as_object_mut()
        .ok_or("page is not an object")?
        .remove("browseContext");
    opened["page"] = inner;
    opened["excludedChildren"] = json!({ "state": "exact", "count": 0 });
    let mut extended = opened.clone();
    extended["futureField"] = added.clone();
    extended["excludedChildren"]["futureField"] = added.clone();
    extended["page"]["rows"][0]["flags"]["boundary"]["futureField"] = added;
    let decoded: BrowseOpenResponse = from_value(extended)?;
    assert_eq!(to_value(decoded)?, opened);
    Ok(())
}

/// `scan.in_progress` explains an undecided value; it is not a failure of a command.
#[test]
fn scan_in_progress_is_a_value_reason_and_not_an_error_envelope_reason() -> TestResult {
    let correlation = Id::parse(ID)?;
    assert_eq!(FolderFlags::REASON_SCAN_IN_PROGRESS, "scan.in_progress");
    assert_eq!(
        FolderFlags::REASON_DATA_INSUFFICIENT,
        "space.data_insufficient"
    );
    let envelope = ErrorEnvelope::from_reason(
        FolderFlags::REASON_SCAN_IN_PROGRESS,
        correlation,
        Vec::new(),
    );
    assert!(envelope.is_none());
    Ok(())
}
