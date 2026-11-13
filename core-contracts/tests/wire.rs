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
fn search_validation_reasons_survive_error_envelope_conversion()
-> Result<(), Box<dyn std::error::Error>> {
    let correlation = Id::parse("018f5e7b-1234-7123-8123-123456789abc")?;
    for reason in [
        "query.syntax_invalid",
        "condition.ambiguous",
        "condition.timezone_unavailable",
        "predicate.unknown",
        "predicate.not_allowed",
        "sort.key_unknown",
    ] {
        let error = ErrorEnvelope::from_reason_or_internal(reason, correlation.clone(), vec![]);
        let body = to_value(error)?;
        assert_eq!(body["code"], "INVALID_ARGUMENT");
        assert_eq!(body["reasonKey"], reason);
        assert_eq!(body["messageKey"], reason);
        assert_eq!(body["correlationId"], correlation.as_str());
        assert_eq!(body["retryable"], false);
    }
    let unknown = ErrorEnvelope::from_reason_or_internal("query.unknown", correlation, vec![]);
    assert_eq!(unknown.code, ErrorCode::Internal);
    assert_eq!(unknown.reason_key, "support.internal_error");
    Ok(())
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
fn space_total_request_rejects_extensions_conflicts_and_arrays()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::analysis_ipc::{SpaceTarget, SpaceTotalRequest};
    let version = "018f0000-0000-7000-8000-000000000002";
    let item = json!({"resultVersion":version,"itemNo":7});
    for target in [
        json!({"kind":"scope_root"}),
        json!({"kind":"folder","item":item}),
        json!({"kind":"query_result","queryRef":version}),
    ] {
        let expected = json!({"target":target,"requestSeq":1});
        assert_eq!(
            to_value(from_value::<SpaceTotalRequest>(expected.clone())?)?,
            expected
        );
    }

    for extended in [
        json!({"target":{"kind":"scope_root"},"requestSeq":1,"futureRequestField":true}),
        json!({"target":{"kind":"scope_root","futureSelectorField":true},"requestSeq":1}),
        json!({"target":{"kind":"folder","item":{"resultVersion":version,"itemNo":7,"futureItemField":true}},"requestSeq":1}),
    ] {
        assert!(from_value::<SpaceTotalRequest>(extended).is_err());
    }
    assert!(from_value::<SpaceTotalRequest>(json!([null,{"kind":"scope_root"},1])).is_err());
    assert!(
        from_value::<SpaceTotalRequest>(json!({
            "target":{"kind":"folder","item":[version,7]},"requestSeq":1
        }))
        .is_err()
    );
    for target in [
        json!({"kind":"scope_root","item":item}),
        json!({"kind":"scope_root","queryRef":null}),
        json!({"kind":"folder","item":null}),
        json!({"kind":"folder","item":item,"queryRef":version}),
        json!({"kind":"query_result","queryRef":version,"item":item}),
        json!({"kind":"future_kind"}),
        json!(["scope_root"]),
    ] {
        assert!(from_value::<SpaceTarget>(target.clone()).is_err());
        assert!(from_value::<SpaceTotalRequest>(json!({"target":target,"requestSeq":1})).is_err());
    }
    Ok(())
}

#[test]
fn space_detail_request_rejects_extensions_and_arrays() -> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::space_detail_ipc::SpaceDetailRequest;
    let version = "018f0000-0000-7000-8000-000000000002";
    let item = json!({"resultVersion":version,"itemNo":7});
    let detail = json!({"resultVersion":version,"item":item});
    assert_eq!(
        to_value(from_value::<SpaceDetailRequest>(detail.clone())?)?,
        detail
    );
    for extended in [
        json!({"resultVersion":version,"item":{"resultVersion":version,"itemNo":7},"futureRequestField":true}),
        json!({"resultVersion":version,"item":{"resultVersion":version,"itemNo":7,"futureItemField":true}}),
    ] {
        assert!(from_value::<SpaceDetailRequest>(extended).is_err());
    }
    assert!(
        from_value::<SpaceDetailRequest>(json!([version,{"resultVersion":version,"itemNo":7}]))
            .is_err()
    );
    assert!(
        from_value::<SpaceDetailRequest>(json!({
            "resultVersion":version,"item":[version,7]
        }))
        .is_err()
    );
    Ok(())
}

#[test]
fn volume_gap_request_rejects_extensions_and_arrays() -> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::volume_gap_ipc::VolumeGapRequest;
    let version = "018f0000-0000-7000-8000-000000000002";
    let volume = PathBytes::from_bytes(&[1]);
    let volume_value = to_value(volume)?;
    let volume_gap = json!({
        "resultVersion":version,"requestSeq":1,"target":{"volumeId":volume_value.clone()}
    });
    assert_eq!(
        to_value(from_value::<VolumeGapRequest>(volume_gap.clone())?)?,
        volume_gap
    );
    for extended in [
        json!({"resultVersion":version,"requestSeq":1,"target":{"volumeId":volume_value.clone()},"futureRequestField":true}),
        json!({"resultVersion":version,"requestSeq":1,"target":{"volumeId":volume_value.clone(),"futureSelectorField":true}}),
        json!({"resultVersion":version,"requestSeq":1,"target":{"root":{"resultVersion":version,"itemNo":7,"futureItemField":true}}}),
    ] {
        assert!(from_value::<VolumeGapRequest>(extended).is_err());
    }
    assert!(
        from_value::<VolumeGapRequest>(json!([version,1,{"volumeId":volume_value.clone()}]))
            .is_err()
    );
    assert!(
        from_value::<VolumeGapRequest>(json!({
            "resultVersion":version,"requestSeq":1,"target":[volume_value.clone(),null]
        }))
        .is_err()
    );
    assert!(
        from_value::<VolumeGapRequest>(json!({
            "resultVersion":version,"requestSeq":1,"target":{"root":[version,7]}
        }))
        .is_err()
    );
    Ok(())
}

#[test]
fn f03_request_bodies_reject_non_objects() -> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::{
        analysis_ipc::SpaceTotalRequest, space_detail_ipc::SpaceDetailRequest,
        volume_gap_ipc::VolumeGapRequest,
    };
    for non_object in [json!([]), json!(null), json!(0), json!("body")] {
        assert!(from_value::<SpaceTotalRequest>(non_object.clone()).is_err());
        assert!(from_value::<SpaceDetailRequest>(non_object.clone()).is_err());
        assert!(from_value::<VolumeGapRequest>(non_object).is_err());
    }
    Ok(())
}

#[test]
fn common_envelopes_and_f03_responses_keep_additive_tolerance()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::{
        CallEnvelope, EventEnvelope, ItemRef, ResultEnvelope, volume_gap_ipc::VolumeGapResponse,
    };
    let id = "018f0000-0000-7000-8000-000000000002";
    let call = from_value::<CallEnvelope>(json!({
        "name":"space.total", "contractVersion":1, "correlationId":id,
        "body":{"target":{"kind":"scope_root"},"requestSeq":1}, "futureEnvelopeField":true,
    }))?;
    assert_eq!(call.name, "space.total");

    let result = from_value::<ResultEnvelope>(json!({
        "name":"space.total", "contractVersion":1, "correlationId":id,
        "body":{"resultVersion":id,"requestSeq":1}, "futureEnvelopeField":true,
    }))?;
    assert_eq!(result.body["requestSeq"], 1);

    let event = from_value::<EventEnvelope>(json!({
        "topic":"results.version_changed", "contractVersion":1, "seq":1,
        "occurredAt":"2026-11-09T12:34:56.123Z", "body":{"resultVersion":id},
        "futureEnvelopeField":true,
    }))?;
    assert!(event.validate());

    let item = from_value::<ItemRef>(json!({
        "resultVersion":id, "itemNo":7, "futureItemField":true,
    }))?;
    assert_eq!(to_value(item)?, json!({"resultVersion":id,"itemNo":7}));

    let path = PathBytes::from_bytes(&[1]);
    let response = from_value::<VolumeGapResponse>(json!({
        "resultVersion":id, "requestSeq":1, "volumeId":path.as_str(),
        "start":{"state":"not_attempted"}, "end":{"state":"not_attempted"},
        "allocated":{"size":{"basis":"allocated","evidence":"unknown","state":"unknown"},"completeness":"complete"},
        "uniquePhysical":{"size":{"basis":"unique_physical","evidence":"unknown","state":"unknown"},"completeness":"complete"},
        "factors":[], "remainder":{"state":"unknown","reasonKey":"space.data_insufficient"},
        "osCategories":{"state":"unavailable","reasonKey":"space.volume_report_unavailable"},
        "futureResponseField":true,
    }))?;
    assert_eq!(
        response
            .volume_id
            .as_ref()
            .and_then(|value| value.bytes().ok()),
        Some(vec![1])
    );
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
