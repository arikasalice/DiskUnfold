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
