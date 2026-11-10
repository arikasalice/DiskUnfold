//! Valid-hash source metadata cases against the combined sealed route.
use super::*;

fn check(value: &serde_json::Value, expected: bool) -> Result<(), String> {
    let mut spool = input(&[1, 0, 0, 0, 0], None, &value.to_string())?;
    let stage = Stage::default();
    let discarded = stage.discarded.clone();
    let result = validate_records_in_sealed(&mut spool, stage, || false);
    assert_eq!(result.is_ok(), expected, "{value}");
    assert_eq!(discarded.load(Ordering::SeqCst), usize::from(!expected));
    Ok(())
}

#[test]
fn stable_source_fields_reject_invalid_shapes_and_accept_open_algorithms() -> Result<(), String> {
    let base: serde_json::Value =
        serde_json::from_str(&example_metadata(&[])?).map_err(|e| e.to_string())?;
    check(&base, true)?;
    for key in [
        "scopeMarker",
        "completeness",
        "conditionSummary",
        "algorithms",
        "pathRules",
    ] {
        let mut value = base.clone();
        value["source"].as_object_mut().ok_or("source")?.remove(key);
        check(&value, false)?;
    }
    for (key, invalid) in [
        ("scopeMarker", serde_json::json!("F".repeat(64))),
        ("scopeMarker", serde_json::json!("a".repeat(63))),
        ("completeness", serde_json::json!("other")),
        (
            "conditionSummary",
            serde_json::json!({"state":"unknown","entries":[]}),
        ),
        ("conditionSummary", serde_json::json!({"state":"known"})),
        (
            "algorithms",
            serde_json::json!({"state":"unknown","entries":[]}),
        ),
        (
            "algorithms",
            serde_json::json!({"state":"known","entries":[{"kind":"a","id":"b","version":0}]}),
        ),
        (
            "algorithms",
            serde_json::json!({"state":"known","entries":[{"kind":"a","id":"b","version":4294967296u64}]}),
        ),
        (
            "algorithms",
            serde_json::json!({"state":"known","entries":[{"kind":"a","id":"b","version":1},{"kind":"a","id":"b","version":1}]}),
        ),
        (
            "pathRules",
            serde_json::json!([{"separator":"/","caseSensitivity":null,"normalization":null}]),
        ),
    ] {
        let mut value = base.clone();
        value["source"][key] = invalid;
        check(&value, false)?;
    }
    let mut value = base.clone();
    value["source"]["scopeMarker"] = serde_json::json!("a".repeat(64));
    value["source"]["completeness"] = serde_json::json!("missing");
    value["source"]["conditionSummary"] = serde_json::json!({"state":"known","entries":[]});
    value["source"]["algorithms"] = serde_json::json!({"state":"known","entries":[{"kind":"future","id":"x","version":4294967295u64},{"kind":"other","id":"x","version":4294967295u64}]});
    check(&value, true)
}

#[test]
fn per_root_rules_are_checked_without_inference() -> Result<(), String> {
    let base: serde_json::Value =
        serde_json::from_str(&example_metadata(&["Lw"])?).map_err(|e| e.to_string())?;
    // No item is needed when the mapping is unknown.
    let volume = [1, 1, 0, 0, 0, 255, 255, 0, 0, 0, 0];
    for (rule, expected) in [
        (
            serde_json::json!({"separator":"/","caseSensitivity":null,"normalization":"NFC"}),
            true,
        ),
        (
            serde_json::json!({"separator":"\\","caseSensitivity":null,"normalization":null}),
            false,
        ),
        (
            serde_json::json!({"separator":"/","caseSensitivity":"unknown","normalization":null}),
            false,
        ),
        (
            serde_json::json!({"separator":"/","caseSensitivity":null}),
            false,
        ),
    ] {
        let mut value = base.clone();
        value["source"]["pathRules"][0] = rule;
        let mut spool = input(&volume, None, &value.to_string())?;
        assert_eq!(
            validate_records_in_sealed(&mut spool, Stage::default(), || false).is_ok(),
            expected
        );
    }
    Ok(())
}
