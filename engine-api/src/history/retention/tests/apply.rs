use super::*;

#[test]
fn apply_is_idempotent_and_cancel_after_apply_has_no_effect() -> Result<(), String> {
    let setup = setup(policy(false, None), Vec::new())?;
    let reviewed = review(&setup, json!({"enabled":true,"days":30}))?;
    let fingerprint = reviewed["fingerprint"]
        .as_str()
        .ok_or("missing fingerprint")?;
    let first = apply(&setup, &reviewed, fingerprint);
    let second = apply(&setup, &reviewed, "00000000000000000000000000000000");
    assert!(first.error.is_none());
    assert_eq!(first.body, second.body);
    assert_eq!(setup.saves.load(Ordering::SeqCst), 1);
    assert_eq!(first.body["outcome"], "applied");
    let cancel = call(
        &setup.engine,
        Channel::Gui,
        "history.retention_cancel",
        json!({"reviewRef": reviewed["reviewRef"]}),
    );
    assert!(cancel.error.is_none());
    assert_eq!(cancel.body["outcome"], "no_effect");
    Ok(())
}

#[test]
fn cancel_is_idempotent_and_leaves_the_policy_unchanged() -> Result<(), String> {
    let initial = policy(false, None);
    let setup = setup(initial.clone(), Vec::new())?;
    let reviewed = review(&setup, json!({"enabled":true,"days":30}))?;
    let request = json!({"reviewRef": reviewed["reviewRef"]});
    let first = call(
        &setup.engine,
        Channel::Gui,
        "history.retention_cancel",
        request.clone(),
    );
    let second = call(
        &setup.engine,
        Channel::Gui,
        "history.retention_cancel",
        request,
    );
    assert_eq!(first.body["outcome"], "cancelled");
    assert_eq!(second.body["outcome"], "no_effect");
    assert_eq!(setup.saves.load(Ordering::SeqCst), 0);
    assert_eq!(
        setup
            .policy
            .lock()
            .map_err(|error| error.to_string())?
            .clone(),
        initial
    );
    Ok(())
}

#[test]
fn save_failure_preserves_policy_and_leaves_review_retryable() -> Result<(), String> {
    let initial = policy(false, None);
    let setup = setup(initial.clone(), Vec::new())?;
    let reviewed = review(&setup, json!({"enabled":true,"days":30}))?;
    setup.save_fails.store(true, Ordering::SeqCst);
    let failed = apply(
        &setup,
        &reviewed,
        reviewed["fingerprint"]
            .as_str()
            .ok_or("missing fingerprint")?,
    );
    let error = failed.error.ok_or("failed save returned success")?;
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(error.reason_key, "history.policy_store_failed");
    assert_eq!(
        setup
            .policy
            .lock()
            .map_err(|error| error.to_string())?
            .clone(),
        initial
    );
    setup.save_fails.store(false, Ordering::SeqCst);
    let retried = apply(
        &setup,
        &reviewed,
        reviewed["fingerprint"]
            .as_str()
            .ok_or("missing fingerprint")?,
    );
    assert!(retried.error.is_none());
    Ok(())
}
