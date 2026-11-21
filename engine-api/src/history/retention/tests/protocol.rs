use super::*;

#[test]
fn eviction_unknown_ids_missing_days_nulls_and_channels_are_rejected() -> Result<(), String> {
    let setup = setup(policy(false, None), Vec::new())?;
    let mut first_ref = None;
    for _ in 0..9 {
        let response = review(&setup, json!({"enabled":false}))?;
        if first_ref.is_none() {
            first_ref = Some(response["reviewRef"].clone());
        }
    }
    let first_ref = first_ref.ok_or("no review was issued")?;
    let evicted = call(
        &setup.engine,
        Channel::Gui,
        "history.retention_cancel",
        json!({"reviewRef": first_ref}),
    );
    assert_eq!(
        evicted
            .error
            .ok_or("old review was not evicted")?
            .reason_key,
        "session.evicted"
    );

    let unknown = call(
        &setup.engine,
        Channel::Gui,
        "history.retention_cancel",
        json!({"reviewRef": id(99)}),
    );
    assert_eq!(
        unknown.error.ok_or("unknown ID was accepted")?.reason_key,
        "history.review_not_found"
    );

    for body in [json!({"enabled":true}), json!({"enabled":true,"days":null})] {
        let invalid = call(
            &setup.engine,
            Channel::Gui,
            "history.retention_review",
            body,
        );
        assert_eq!(
            invalid.error.ok_or("invalid request was accepted")?.code,
            ErrorCode::InvalidArgument
        );
    }
    let channel_review = review(&setup, json!({"enabled":true,"days":30}))?;
    for channel in [Channel::Cli, Channel::Mcp] {
        for (command, body) in [
            (
                "history.retention_review",
                json!({"enabled":true,"days":30}),
            ),
            (
                "history.retention_apply",
                json!({
                    "reviewRef": channel_review["reviewRef"],
                    "fingerprint": channel_review["fingerprint"]
                }),
            ),
            (
                "history.retention_cancel",
                json!({"reviewRef": channel_review["reviewRef"]}),
            ),
        ] {
            let denied = call(&setup.engine, channel, command, body);
            assert_eq!(
                denied.error.ok_or("non-GUI channel was accepted")?.code,
                ErrorCode::InvalidArgument,
                "{channel:?} {command}"
            );
        }
    }
    Ok(())
}

#[test]
fn review_fingerprint_uses_the_versioned_canonical_record() -> Result<(), EngineError> {
    let target_ids = [id(2), id(1)];
    let mut sorted_ids = target_ids
        .iter()
        .map(|job_id| job_id.v7_bytes().ok_or_else(count_unavailable))
        .collect::<Result<Vec<_>, _>>()?;
    sorted_ids.sort_unstable();
    let mut target_hasher = blake3::Hasher::new();
    for job_id in sorted_ids {
        target_hasher.update(&job_id);
    }
    let target_digest = lower_hex(target_hasher.finalize().as_bytes());
    assert_eq!(digest_ids(&target_ids)?, target_digest);
    let expected_json = format!(
        "{{\"changeKind\":\"enable\",\"confirmationRequired\":true,\"days\":30,\"deleteGeneration\":\"7\",\"enabled\":true,\"targets\":\"{target_digest}\"}}"
    );
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"diskunfold.history.retention-review.v1\0");
    hasher.update(expected_json.as_bytes());
    assert_eq!(
        review_fingerprint(
            HistoryRetentionChangeKind::Enable,
            true,
            Some(30),
            7,
            true,
            &target_digest,
        ),
        lower_hex(&hasher.finalize().as_bytes()[..16])
    );
    Ok(())
}
