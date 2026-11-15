//! The receipt's hexadecimal scope fingerprint is the 32-byte recent-scope key, in text form.
use super::*;
use du_analysis_index::fingerprint_from_hex;
use du_core_contracts::analysis_ipc::{ScanStart, ScopeDraft};

#[test]
fn a_scope_with_exclusion_rules_keeps_one_fingerprint_from_receipt_to_stored_key()
-> Result<(), Box<dyn std::error::Error>> {
    let comparison: Arc<dyn PathComparison> = Arc::new(ProvenComparison);
    let harness = super::fixture::scan_harness_with_comparison(None, Some(comparison.clone()))?;
    let draft: ScopeDraft = serde_json::from_value(json!({
        "includes": [native("C:\\Fixture")],
        "exclusions": [{"kind": "path", "path": native("C:\\Fixture\\Private")}],
        "boundary": "current_volume",
        "includedMounts": [],
        "source": "gui"
    }))?;
    let gui = |name, body| call(&harness.engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let review: ScopeReview = serde_json::from_value(
        gui("scope.review", json!({"draft": draft, "requestSeq": 1}))?.body,
    )?;
    let receipt: ScanStart = serde_json::from_value(
        gui(
            "scan.start",
            json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
        )?
        .body,
    )?;
    // The key recent scopes store is computed from the admitted draft's comparison inputs.
    let verified = crate::scope::verify(
        &draft,
        &review.review_fingerprint,
        &*harness.roots,
        Some(comparison),
        false,
    )
    .map_err(|error| format!("{error:?}"))?;
    let hashed = verified
        .scope_fingerprint
        .retained_comparison_input()
        .ok_or("a scope with rules must retain its comparison inputs")?;
    assert!(!hashed.exclusions.is_empty());
    let key =
        crate::scope::recent_scope_fingerprint_v1(&hashed).map_err(|error| format!("{error:?}"))?;
    assert_eq!(fingerprint_from_hex(&receipt.scope_fingerprint), Some(key));
    assert_eq!(
        blake3::Hash::from_bytes(key).to_hex().as_str(),
        receipt.scope_fingerprint
    );
    // The finished scan's sealed result carries the same text, so the observation row
    // is keyed exactly like the recent scope.
    harness.release.send(())?;
    let (summary, _) = scan_finished(&harness.terminal)?;
    let version = harness
        .versions
        .sealed_scan_result(summary.result_version_id.as_ref().ok_or("missing result")?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        version.scope_fingerprint(),
        Some(receipt.scope_fingerprint.as_str())
    );
    Ok(())
}
