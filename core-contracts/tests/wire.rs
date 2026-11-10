#![allow(clippy::unwrap_used)]
use du_core_contracts::*;
use serde_json::{from_value, json, to_value};

#[test]
fn sizes_preserve_u64_and_reject_invalid_values() {
    for value in ["0", "18446744073709551615"] {
        let b: ByteSize = from_value(json!(value)).unwrap();
        assert_eq!(to_value(b).unwrap(), json!(value));
    }
    for value in [
        json!(0),
        json!("18446744073709551616"),
        json!("-1"),
        json!("01"),
    ] {
        assert!(from_value::<ByteSize>(value).is_err());
    }
    let unknown = json!({"basis":"logical","evidence":"unknown","state":"unknown"});
    assert!(from_value::<SizeValue>(unknown.clone()).is_ok());
    assert!(
        from_value::<SizeValue>(
            json!({"basis":"logical","evidence":"unknown","state":"unknown","bytes":"0"})
        )
        .is_err()
    );
    assert_ne!(
        unknown,
        json!({"basis":"logical","evidence":"unknown","state":"exact","bytes":"0"})
    );
}
#[test]
fn ids_and_timestamps_validate_wire_input() {
    assert!(Id::parse("018f5e7b-1234-7123-8123-123456789abc").is_ok());
    assert!(Id::parse("018f5e7b-1234-4123-8123-123456789abc").is_err());
    assert!(Timestamp::parse("2026-11-09T12:34:56.123Z").is_ok());
    for value in [
        "2026-11-09T12:34:56.123456Z",
        "2026-11-09T12:34:56.123+00:00",
        "2026-02-30T12:34:56.123Z",
    ] {
        assert!(Timestamp::parse(value).is_err());
    }
}
#[test]
fn paths_round_trip_raw_bytes() {
    for bytes in [vec![0xff, b'/'], vec![0x00, 0xd8]] {
        let raw = PathBytes::from_bytes(&bytes);
        assert_eq!(raw.bytes().unwrap(), bytes);
        let encoded = to_value(raw).unwrap();
        assert_eq!(
            from_value::<PathBytes>(encoded).unwrap().bytes().unwrap(),
            bytes
        );
    }
    assert!(from_value::<PathBytes>(json!("/w==")).is_err());
}
#[test]
fn errors_and_event_bounds() {
    let id = Id::parse("018f5e7b-1234-7123-8123-123456789abc").unwrap();
    let error = ErrorEnvelope::from_reason("db.busy", id.clone(), vec![]).unwrap();
    let wire = to_value(error).unwrap();
    assert_eq!(wire["retryable"], true);
    assert_eq!(wire["messageKey"], "db.busy");
    assert_eq!(wire["reasonKey"], "db.busy");
    assert_ne!(wire["reasonKey"], "support.internal_error");
    assert!(wire.get("itemRefs").is_none());
    let mut event = EventEnvelope {
        topic: "job.finished".into(),
        contract_version: 1,
        seq: 1,
        correlation_id: Some(id),
        job_ref: None,
        result_version: None,
        occurred_at: Timestamp::parse("2026-11-09T12:34:56.123Z").unwrap(),
        body: json!({}),
    };
    assert!(event.validate());
    event.seq = 1 << 53;
    assert!(!event.validate());
    event.seq = 0;
    assert!(!event.validate());
}

#[test]
fn scope_review_rejects_unknown_fields_and_keeps_native_paths() {
    use du_core_contracts::analysis_ipc::{ScopeDraft, ScopeReviewRequest};
    let draft = json!({
        "includes": [{"encoding": "windows_utf16le", "raw": "ANg", "display": "�", "lossy": true}],
        "exclusions": [], "boundary": "current_volume", "includedMounts": [], "source": "gui",
    });
    let parsed: ScopeDraft = from_value(draft.clone()).unwrap();
    assert_eq!(parsed.includes[0].raw.bytes().unwrap(), [0, 0xd8]);
    assert_eq!(to_value(parsed).unwrap(), draft);
    assert!(
        from_value::<ScopeReviewRequest>(json!({"draft":draft,"requestSeq":1,"typo":1})).is_err()
    );
}

#[test]
fn space_total_accepts_extensions_without_accepting_conflicting_selectors()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::analysis_ipc::SpaceTotalRequest;
    let version = "018f0000-0000-7000-8000-000000000002";
    let item = json!({"resultVersion":version,"itemNo":7});
    for target in [
        json!({"kind":"scope_root"}),
        json!({"kind":"folder","item":item}),
        json!({"kind":"query_result","queryRef":version}),
    ] {
        let expected = json!({"target":target,"requestSeq":1});
        let mut extended = expected.clone();
        extended["futureRequestField"] = json!(true);
        extended["target"]["futureTargetField"] = json!({"data":3});
        assert_eq!(
            to_value(from_value::<SpaceTotalRequest>(extended)?)?,
            expected
        );
    }
    for target in [
        json!({"kind":"scope_root","item":item}),
        json!({"kind":"scope_root","queryRef":null}),
        json!({"kind":"folder","item":null}),
        json!({"kind":"folder","item":item,"queryRef":version}),
        json!({"kind":"query_result","queryRef":version,"item":item}),
        json!({"kind":"query_result","futureTargetField":version}),
        json!({"kind":"future_kind"}),
        json!(["scope_root"]),
    ] {
        assert!(from_value::<SpaceTotalRequest>(json!({"target":target,"requestSeq":1})).is_err());
    }
    Ok(())
}

#[test]
fn job_status_serializes_query_reference_separately_from_result_version()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::analysis_ipc::AnalysisJobStatus;
    let job = "018f0000-0000-7000-8000-000000000001";
    let query = "018f0000-0000-7000-8000-000000000002";
    let input = serde_json::json!({"jobRef":job,"kind":"query.evaluate","state":"done","sequence":2,"resultVersion":null,"queryRef":query,"reasonKey":null});
    let status: AnalysisJobStatus = serde_json::from_value(input.clone())?;
    assert_eq!(serde_json::to_value(status)?, input);
    let mut without = input;
    without
        .as_object_mut()
        .ok_or("not an object")?
        .remove("queryRef");
    let status: AnalysisJobStatus = serde_json::from_value(without.clone())?;
    assert_eq!(serde_json::to_value(status)?, without);
    Ok(())
}
