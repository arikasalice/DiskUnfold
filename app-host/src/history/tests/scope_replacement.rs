//! Exercises scope revalidation through desktop IPC and the native Windows adapter.
use super::*;
use du_core_contracts::{SizeAmount, SizeBasis, analysis_ipc::SpaceTotal};
use std::fs;

#[test]
fn same_path_replacement_requires_a_new_review_before_scanning()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let target = fixture.path().join("reviewed");
    fs::create_dir(&target)?;
    fs::write(target.join("original.bin"), [1; 4096])?;

    let database = crate::startup_database::open(profile.path())?;
    let reader = HistoryReader::open(profile.path(), database.clone())?;
    let random = &du_platform_os::entropy::OsRandom;
    let mut builder = EngineBuilder::default();
    let events = du_engine_api::desktop::register(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        random,
    )?;
    du_engine_api::history::register(&mut builder, reader, random)
        .map_err(|error| format!("{error:?}"))?;
    let engine = builder.build()?;
    let raw: Vec<_> = target
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({
        "includes": [{
            "encoding": "windows_utf16le", "raw": PathBytes::from_bytes(&raw),
            "display": target, "lossy": false
        }],
        "exclusions": [], "boundary": "current_volume", "includedMounts": [], "source": "gui"
    });
    let review = |seq| -> Result<ScopeReview, Box<dyn std::error::Error>> {
        let response = ipc(
            &engine,
            "scope.review",
            json!({"draft": draft, "requestSeq": seq}),
        )?;
        assert!(response.error.is_none(), "{:?}", response.error);
        Ok(serde_json::from_value(response.body)?)
    };
    let start = |review: &ScopeReview| {
        ipc(
            &engine,
            "scan.start",
            json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
        )
    };

    let original = review(1)?;
    // Retain the original directory so Windows cannot reuse its live identity.
    fs::rename(&target, fixture.path().join("prior"))?;
    fs::create_dir(&target)?;
    fs::write(target.join("replacement.bin"), [2; 16_384])?;

    let rejected = start(&original)?.error.ok_or("stale review was accepted")?;
    assert_eq!(rejected.code, ErrorCode::StaleVersion);
    assert_eq!(rejected.reason_key, "scope.changed");
    let history: HistoryListResponse = serde_json::from_value(call(&engine, json!({}))?.body)?;
    assert!(history.rows.is_empty());

    let replacement = review(2)?;
    assert_ne!(original.review_fingerprint, replacement.review_fingerprint);
    let accepted = start(&replacement)?;
    assert!(accepted.error.is_none(), "{:?}", accepted.error);
    let scan: ScanStart = serde_json::from_value(accepted.body)?;
    let batch = terminal_batch(&events, &scan.job_ref)?;
    let finished = batch
        .iter()
        .find(|event| {
            event.topic == "job.finished" && event.job_ref.as_ref() == Some(&scan.job_ref)
        })
        .ok_or("missing scan completion")?;
    assert_eq!(finished.body["state"], "done");
    let version = finished
        .result_version
        .as_ref()
        .ok_or("missing result version")?;
    let response = ipc(
        &engine,
        "space.total",
        json!({"resultVersion": version, "requestSeq": 3, "target": {"kind": "scope_root"}}),
    )?;
    assert!(response.error.is_none(), "{:?}", response.error);
    let total: SpaceTotal = serde_json::from_value(response.body)?;
    assert_eq!(&total.result_version, version);
    let logical = total
        .sizes
        .iter()
        .find(|entry| entry.size.basis == SizeBasis::Logical)
        .ok_or("missing logical total")?;
    assert_eq!(
        logical.size.amount,
        SizeAmount::Exact {
            bytes: 16_384_u64.into()
        }
    );
    Ok(())
}
