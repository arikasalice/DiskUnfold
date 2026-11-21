use super::*;

#[test]
fn review_classifies_every_published_change_kind() -> Result<(), String> {
    let cases = [
        (
            policy(false, None),
            json!({"enabled":false}),
            "none",
            false,
            false,
        ),
        (
            policy(true, Some(30)),
            json!({"enabled":true,"days":30}),
            "none",
            false,
            false,
        ),
        (
            policy(false, None),
            json!({"enabled":true,"days":30}),
            "enable",
            true,
            true,
        ),
        (
            policy(true, Some(30)),
            json!({"enabled":true,"days":14}),
            "shorten",
            true,
            true,
        ),
        (
            policy(true, Some(30)),
            json!({"enabled":true,"days":90}),
            "extend",
            true,
            false,
        ),
        (
            policy(true, Some(30)),
            json!({"enabled":false}),
            "disable",
            true,
            false,
        ),
        (
            policy(false, None),
            json!({"enabled":false,"days":60}),
            "set_period",
            true,
            false,
        ),
    ];
    for (current, request, kind, confirmation, cleanup) in cases {
        let setup = setup(current, Vec::new())?;
        let response = review(&setup, request)?;
        assert_eq!(response["changeKind"], kind);
        assert_eq!(response["confirmationRequired"], confirmation);
        assert_eq!(response["cleanupAfterApply"], cleanup);
        assert_eq!(response["targetCount"], "0");
        assert_eq!(response["heldCount"], "0");
        assert_eq!(response["undoInfoRemovalCount"], "0");
        assert!(response.get("targetEarliestAtMs").is_none());
        assert!(response.get("targetLatestAtMs").is_none());
    }
    Ok(())
}

#[test]
fn review_counts_follow_up_and_undo_holds_and_uses_target_range() -> Result<(), String> {
    let candidates = vec![
        candidate(1, 100, false),
        candidate(2, 50, false),
        candidate(3, 25, false),
        candidate(4, 10, true),
        candidate(5, 80, false),
    ];
    let setup = setup(policy(false, None), candidates)?;
    for seed in [2, 3, 5] {
        setup
            .undo
            .present
            .lock()
            .map_err(|error| error.to_string())?
            .insert(id(seed));
    }
    setup
        .undo
        .in_use
        .lock()
        .map_err(|error| error.to_string())?
        .insert(id(2));
    setup
        .undo
        .in_use_errors
        .lock()
        .map_err(|error| error.to_string())?
        .insert(id(3));
    let response = review(&setup, json!({"enabled":true,"days":30}))?;
    assert_eq!(response["targetCount"], "2");
    assert_eq!(response["heldCount"], "3");
    assert_eq!(response["undoInfoRemovalCount"], "1");
    assert_eq!(response["targetEarliestAtMs"], 80);
    assert_eq!(response["targetLatestAtMs"], 100);
    Ok(())
}

#[test]
fn undo_port_failure_holds_candidates_without_failing_the_review() -> Result<(), String> {
    let setup = setup(policy(false, None), vec![candidate(1, 50, false)])?;
    setup.undo.unavailable.store(true, Ordering::SeqCst);
    let response = review(&setup, json!({"enabled":true,"days":30}))?;
    assert_eq!(response["targetCount"], "0");
    assert_eq!(response["heldCount"], "1");
    Ok(())
}

#[test]
fn count_failure_is_internal_and_never_returns_zero() -> Result<(), String> {
    let setup = setup(policy(false, None), vec![candidate(1, 50, false)])?;
    setup.reader.lock().map_err(|error| error.to_string())?.fail = true;
    let result = call(
        &setup.engine,
        Channel::Gui,
        "history.retention_review",
        json!({"enabled":true,"days":30}),
    );
    let error = result
        .error
        .ok_or("count failure returned a success body")?;
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(error.reason_key, "history.count_unavailable");
    assert!(result.body.is_null());
    Ok(())
}

#[test]
fn stale_fingerprint_keeps_the_review_available_for_retry() -> Result<(), String> {
    let setup = setup(policy(false, None), vec![candidate(1, 50, false)])?;
    let reviewed = review(&setup, json!({"enabled":true,"days":30}))?;
    let ref_value = review_ref(&reviewed)?;
    let wrong = apply(&setup, &reviewed, "ffffffffffffffffffffffffffffffff");
    let stale = wrong.error.ok_or("wrong fingerprint was applied")?;
    assert_eq!(stale.code, ErrorCode::StaleVersion);
    assert_eq!(stale.reason_key, "history.review_stale");
    assert_eq!(review_ref(&reviewed)?, ref_value);
    let correct = apply(
        &setup,
        &reviewed,
        reviewed["fingerprint"]
            .as_str()
            .ok_or("missing fingerprint")?,
    );
    assert!(correct.error.is_none());
    Ok(())
}

#[test]
fn recount_detects_generation_change_and_keeps_the_review() -> Result<(), String> {
    let setup = setup(policy(false, None), vec![candidate(1, 50, false)])?;
    let reviewed = review(&setup, json!({"enabled":true,"days":30}))?;
    setup
        .reader
        .lock()
        .map_err(|error| error.to_string())?
        .delete_generation = 8;
    let stale = apply(
        &setup,
        &reviewed,
        reviewed["fingerprint"]
            .as_str()
            .ok_or("missing fingerprint")?,
    );
    let error = stale.error.ok_or("changed generation was accepted")?;
    assert_eq!(error.reason_key, "history.review_stale");
    setup
        .reader
        .lock()
        .map_err(|error| error.to_string())?
        .delete_generation = 7;
    assert!(
        apply(
            &setup,
            &reviewed,
            reviewed["fingerprint"]
                .as_str()
                .ok_or("missing fingerprint")?,
        )
        .error
        .is_none()
    );
    Ok(())
}
