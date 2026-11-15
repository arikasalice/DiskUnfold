use super::*;

#[test]
fn invalid_rule_and_invalid_native_path_keep_existing_reasons()
-> Result<(), Box<dyn std::error::Error>> {
    let ScanHarness { engine, .. } =
        fixture::scan_harness_with_comparison(None, Some(Arc::new(ProvenComparison)))?;
    let gui = |body| {
        call(
            &engine,
            "scope.review",
            Channel::Gui,
            ENVELOPE_VERSION,
            body,
        )
    };
    let mut draft = json!({"includes":[native("C:\\Fixture")],"exclusions":[{"kind":"name_pattern","pattern":"*.tmp"}],"boundary":"current_volume","includedMounts":[],"source":"gui"});
    assert_eq!(
        gui(json!({"draft":draft,"requestSeq":1}))?
            .error
            .ok_or("invalid rule accepted")?
            .reason_key,
        "scope.exclude_invalid"
    );
    draft["exclusions"][0] = json!({"kind":"path","path":native("C:\\Fixture\\F")});
    draft["exclusions"][0]["path"]["encoding"] = json!("unix");
    assert_eq!(
        gui(json!({"draft":draft,"requestSeq":2}))?
            .error
            .ok_or("invalid path accepted")?
            .reason_key,
        "path.unresolvable"
    );
    // A well-formed Unix rule cannot be compared with a Windows scope.
    draft["exclusions"][0]["path"] = json!({"encoding": "unix", "raw": PathBytes::from_bytes(b"/Fixture/F"), "display": "/Fixture/F", "lossy": false});
    assert_eq!(
        gui(json!({"draft":draft,"requestSeq":3}))?
            .error
            .ok_or("mixed encodings accepted")?
            .reason_key,
        "scope.exclude_invalid"
    );
    Ok(())
}

/// A path inside a request draft may not carry a field this build does not define.
#[test]
fn unknown_fields_in_request_paths_are_an_invalid_body() -> Result<(), Box<dyn std::error::Error>> {
    let ScanHarness { engine, .. } =
        fixture::scan_harness_with_comparison(None, Some(Arc::new(ProvenComparison)))?;
    let review = |draft: &serde_json::Value| {
        let body = json!({"draft": draft, "requestSeq": 1});
        call(
            &engine,
            "scope.review",
            Channel::Gui,
            ENVELOPE_VERSION,
            body,
        )
    };
    let reason = |draft: &serde_json::Value| review(draft).map(|r| r.error.map(|e| e.reason_key));
    let reviewed = json!({"includes":[native("C:\\Fixture")],"exclusions":[{"kind":"path","path":native("C:\\Fixture\\F")}],"boundary":"current_volume","includedMounts":[],"source":"gui"});
    assert_eq!(reason(&reviewed)?, None);
    let mut with_mount = reviewed.clone();
    with_mount["boundary"] = json!("include_selected");
    with_mount["includedMounts"] = json!([native("C:\\Fixture\\M")]);
    assert_ne!(
        reason(&with_mount)?.as_deref(),
        Some("command.body_invalid")
    );
    for (draft, pointer) in [
        (&reviewed, "/includes/0"),
        (&reviewed, "/exclusions/0/path"),
        (&with_mount, "/includedMounts/0"),
    ] {
        let mut extended = draft.clone();
        extended
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or(pointer)?
            .insert("futureField".into(), json!(true));
        assert_eq!(
            reason(&extended)?.as_deref(),
            Some("command.body_invalid"),
            "{pointer}"
        );
    }
    Ok(())
}
