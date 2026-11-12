use super::{
    LocaleBundleRequest, LocaleBundleResponse, LocaleMissingEntry, LocaleReportMissingRequest,
    LocaleReportMissingResponse,
};
use serde_json::json;

#[test]
fn report_missing_requires_object_shapes_and_exact_request_fields() {
    let request = json!({
        "language": "ko",
        "entries": [{"key": "screen.title", "marker": "fallback"}]
    });
    let parsed = serde_json::from_value::<LocaleReportMissingRequest>(request.clone());
    assert!(parsed.is_ok());

    for invalid in [
        json!(["ko", [{"key": "screen.title", "marker": "fallback"}]]),
        json!({"language": null, "entries": []}),
        json!({"language": "ko", "entries": null}),
        json!({"language": "ko", "entries": [], "extra": true}),
        json!({"language": "ko", "entries": [["screen.title", "fallback"]]}),
        json!({
            "language": "ko",
            "entries": [{"key": "screen.title", "marker": "key", "value": "secret"}]
        }),
        json!({
            "language": "ko",
            "entries": [{"key": null, "marker": "key"}]
        }),
        json!({
            "language": "ko",
            "entries": [{"key": "screen.title", "marker": null}]
        }),
        json!({
            "language": "ko",
            "entries": [{"key": "screen.title", "marker": "unknown"}]
        }),
    ] {
        assert!(
            serde_json::from_value::<LocaleReportMissingRequest>(invalid).is_err(),
            "invalid report body must be rejected"
        );
    }

    assert!(serde_json::from_value::<LocaleMissingEntry>(json!([])).is_err());
    assert!(serde_json::from_value::<LocaleReportMissingResponse>(json!({})).is_ok());
    assert!(
        serde_json::from_value::<LocaleReportMissingResponse>(json!({
            "futureField": true
        }))
        .is_ok()
    );
}

#[test]
fn bundle_revision_fields_reject_null_and_bundle_body_is_omitted_only() {
    assert!(serde_json::from_value::<LocaleBundleRequest>(json!({"language":"en"})).is_ok());
    assert!(
        serde_json::from_value::<LocaleBundleRequest>(json!({
            "language":"en",
            "knownBundleRevision":null
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<LocaleBundleRequest>(json!({
            "language":"en",
            "extra":true
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<LocaleBundleResponse>(json!({
            "bundleRevision":"opaque"
        }))
        .is_ok()
    );
    assert!(
        serde_json::from_value::<LocaleBundleResponse>(json!({
            "bundleRevision":"opaque",
            "bundle":null
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<LocaleBundleResponse>(json!({
            "bundleRevision":"opaque",
            "futureField":true
        }))
        .is_ok()
    );
}
