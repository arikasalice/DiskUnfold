//! Scope review shows display paths while admission keeps the native root.
use super::*;

fn draft(raw: &[u8]) -> serde_json::Value {
    json!({
        "includes": [{
            "encoding": "windows_utf16le",
            "raw": PathBytes::from_bytes(raw),
            "display": "untrusted",
            "lossy": false
        }],
        "exclusions": [],
        "boundary": "current_volume",
        "includedMounts": [],
        "source": "gui"
    })
}

#[test]
fn review_shows_the_drive_form_and_admission_keeps_native_values()
-> Result<(), Box<dyn std::error::Error>> {
    let raw: Vec<u8> = r"\\?\C:\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let harness = scan_harness_with_recent_admissions()?;
    let admissions = harness
        .recent_scope_admissions
        .as_ref()
        .ok_or("recent admission queue missing")?;
    let call = |name, body| call(&harness.engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let reviewed = call(
        "scope.review",
        json!({"draft": draft(&raw), "requestSeq": 1}),
    )?;
    let review: ScopeReview = serde_json::from_value(reviewed.body)?;
    for path in review
        .scope
        .includes
        .iter()
        .chain(review.targets.iter().map(|target| &target.path))
    {
        assert_eq!(path.display, r"C:\Fixture");
        assert_eq!(path.raw, PathBytes::from_bytes(&raw));
        assert!(!path.lossy);
    }
    // The GUI resubmits the reviewed scope; its projected text must not matter.
    let started = call(
        "scan.start",
        json!({"draft": review.scope, "reviewFingerprint": review.review_fingerprint}),
    )?;
    assert!(started.error.is_none(), "{:?}", started.error);
    let receipt: ScanStart = serde_json::from_value(started.body)?;
    let ticket = admissions
        .delivery()
        .claim(&receipt.job_ref)
        .ok_or("missing delivery receipt")?;
    ticket.posted();
    let admission = admissions.try_next()?;
    let stored = &admission.normalized_scope().includes[0];
    assert_eq!(stored.display, r"\\?\C:\Fixture");
    assert_eq!(stored.raw, PathBytes::from_bytes(&raw));
    let fingerprint = crate::scope::recent_scope_fingerprint_v1(admission.fingerprint_conditions())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        blake3::Hash::from_bytes(fingerprint).to_hex().as_str(),
        receipt.scope_fingerprint
    );
    harness.release.send(())?;
    let (summary, _) = scan_finished(&harness.terminal)?;
    assert_eq!(summary.job_id, receipt.job_ref);
    Ok(())
}
