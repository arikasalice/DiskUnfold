use super::*;

#[test]
fn two_reviewed_paths_publish_one_result_and_denied_paths_never_start()
-> Result<(), Box<dyn std::error::Error>> {
    let ScanHarness {
        engine,
        roots,
        release,
        terminal,
        versions,
    } = scan_harness()?;
    let gui = |name, body| call(&engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let native = |path: &str| {
        let bytes: Vec<_> = path.encode_utf16().flat_map(u16::to_le_bytes).collect();
        json!({"encoding": "windows_utf16le", "raw": PathBytes::from_bytes(&bytes), "display": "", "lossy": false})
    };
    let draft = json!({
        "includes": [native("D:\\Other"), native("C:\\Fixture"), native("C:\\Fixture")],
        "exclusions": [], "boundary": "current_volume", "includedMounts": [], "source": "gui"
    });
    let reviewed = gui("scope.review", json!({"draft": draft, "requestSeq": 10}))?;
    assert!(reviewed.error.is_none());
    let review: ScopeReview = serde_json::from_value(reviewed.body)?;
    assert_eq!(review.scope.includes.len(), 2);
    assert_eq!(review.targets.len(), 3);
    assert!(
        review
            .notices
            .iter()
            .any(|notice| notice == "scope.overlap_merged")
    );
    let mut denied = draft.clone();
    denied["includes"][0] = native("Z:\\Denied");
    for (method, body, reason) in [
        (
            "scope.review",
            json!({"draft": denied, "requestSeq": 11}),
            "scope.target_unavailable",
        ),
        (
            "scan.start",
            json!({"draft": denied, "reviewFingerprint": review.review_fingerprint}),
            "scope.changed",
        ),
    ] {
        assert_eq!(
            gui(method, body)?
                .error
                .ok_or("denied scope accepted")?
                .reason_key,
            reason
        );
    }
    assert_eq!(roots.reads.load(Ordering::Acquire), 0);
    let receipt: ScanStart = serde_json::from_value(
        gui(
            "scan.start",
            json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
        )?
        .body,
    )?;
    release.send(())?;
    release.send(())?;
    let (summary, _) = scan_finished(&terminal)?;
    assert_eq!(summary.job_id, receipt.job_ref);
    assert_eq!(summary.scope_summary, "metadata: 2 roots");
    let version = summary.result_version_id.ok_or("missing forest result")?;
    let pinned = versions
        .pin_current(&version)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(pinned.observed_roots().len(), 2);
    let total = gui(
        "space.total",
        json!({
            "resultVersion": version, "target": {"kind": "scope_root"}, "requestSeq": 12
        }),
    )?;
    assert_eq!(total.body["sizes"][0]["size"]["bytes"], "128");
    let volume_id = PathBytes::from_bytes(&[1]);
    let root = json!({"resultVersion":version,"itemNo":1});
    for target in [json!({"root":root}), json!({"volumeId":volume_id})] {
        let report = gui(
            "space.volume_gap",
            json!({"resultVersion":version,"target":target,"requestSeq":13}),
        )?;
        assert!(report.error.is_none(), "{:?}", report.error);
        assert_eq!(report.body["requestSeq"], 13);
        assert_eq!(report.body["volumeId"], volume_id.as_str());
        assert_eq!(report.body["start"]["state"], "unavailable");
        assert_eq!(report.body["end"]["state"], "not_attempted");
        assert_eq!(report.body["factors"].as_array().map(Vec::len), Some(8));
        assert_eq!(
            report.body["remainder"]["reasonKey"],
            "space.volume_report_unavailable"
        );
        assert_eq!(report.body["osCategories"]["state"], "unavailable");
    }
    for (target, reason) in [
        (
            json!({"root":root,"volumeId":volume_id}),
            "space.target_out_of_scope",
        ),
        (json!({}), "space.target_out_of_scope"),
        (
            json!({"root":{"resultVersion":version,"itemNo":u32::MAX}}),
            "space.target_out_of_scope",
        ),
        (
            json!({"root":{"resultVersion":receipt.scope_version,"itemNo":0}}),
            "result.requery_required",
        ),
        (
            json!({"root":root,"path":"ignored"}),
            "command.body_invalid",
        ),
    ] {
        assert_eq!(
            gui(
                "space.volume_gap",
                json!({"resultVersion":version,"target":target,"requestSeq":14}),
            )?
            .error
            .ok_or("invalid volume request was accepted")?
            .reason_key,
            reason
        );
    }
    assert_eq!(
        call(
            &engine,
            "space.volume_gap",
            Channel::Mcp,
            ENVELOPE_VERSION,
            json!({"resultVersion":version,"target":{"root":root},"requestSeq":15})
        )?
        .error
        .ok_or("volume query was exposed to MCP")?
        .reason_key,
        "command.not_provided"
    );
    assert_eq!(roots.reads.load(Ordering::Acquire), 4);
    Ok(())
}
