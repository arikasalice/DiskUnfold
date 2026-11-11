use super::*;

pub(super) fn check_uncomposed_scope_fails_closed(
    engine: &Engine,
    roots: &Roots,
    draft: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let gui = |name, body| call(engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    // Unsupported exclusions or mount traversal must never be silently ignored.
    for (field, value, reason) in [
        (
            "boundary",
            json!("include_selected"),
            "scope.target_unavailable",
        ),
        (
            "exclusions",
            json!([{"kind": "name_pattern", "pattern": "*.tmp"}]),
            "scope.exclude_invalid",
        ),
        (
            "exclusions",
            json!([{"kind": "path", "path": draft["includes"][0]}]),
            "scope.exclude_invalid",
        ),
    ] {
        let mut unsupported = draft.clone();
        unsupported[field] = value;
        let rejected_review = gui(
            "scope.review",
            json!({"draft": unsupported, "requestSeq": 0}),
        )?;
        assert_eq!(
            rejected_review
                .error
                .ok_or("unsupported review unexpectedly succeeded")?
                .reason_key,
            reason
        );
        let rejected_start = gui(
            "scan.start",
            json!({"draft": unsupported, "reviewFingerprint": "unreviewed"}),
        )?;
        assert_eq!(
            rejected_start
                .error
                .ok_or("unsupported scan unexpectedly started")?
                .reason_key,
            reason
        );
    }
    assert_eq!(roots.reads.load(Ordering::Acquire), 0);
    assert_eq!(roots.opens.load(Ordering::Acquire), 0);
    Ok(())
}
