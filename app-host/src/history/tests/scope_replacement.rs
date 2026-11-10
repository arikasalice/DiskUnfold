//! Exercises scope revalidation through desktop IPC and the native Windows adapter.
use super::*;
use du_core_contracts::{
    SizeAmount, SizeBasis,
    analysis_ipc::SpaceTotal,
    browse_ipc::BrowseValue,
    browse_page_ipc::{BrowseOpenResponse, QueryPageResponse},
    query_region_ipc::QueryRegionOpened,
};
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
    verify_pinned_folder(&engine, &events, version)?;
    Ok(())
}

/// Opens the scanned fixture through desktop IPC and checks the region lifetime.
fn verify_pinned_folder(
    engine: &Engine,
    events: &du_engine_api::desktop::DesktopEvents,
    version: &Id,
) -> Result<(), Box<dyn std::error::Error>> {
    let issued = ipc(engine, "query.region.open", json!({"kind": "browse_list"}))?;
    assert!(issued.error.is_none(), "{:?}", issued.error);
    let QueryRegionOpened { region_ref } = serde_json::from_value(issued.body)?;
    let open = |seq| {
        ipc(
            engine,
            "browse.open",
            json!({"regionRef": region_ref, "viewKind": "list", "requestSeq": seq,
                "resultVersion": version}),
        )
    };
    let first = open(1)?;
    assert!(first.error.is_none(), "{:?}", first.error);
    let first = serde_json::from_value(first.body)?;
    let completed = match first {
        BrowseOpenResponse::Job { job_ref, .. } => {
            let batch = terminal_batch(events, &job_ref)?;
            assert!(batch.iter().any(|event| {
                event.topic == "job.finished"
                    && event.job_ref.as_ref() == Some(&job_ref)
                    && event.body["state"] == "done"
            }));
            // The registered result is available even when the first request
            // reached the asynchronous path under load.
            let cached = open(2)?;
            assert!(cached.error.is_none(), "{:?}", cached.error);
            serde_json::from_value(cached.body)?
        }
        opened => opened,
    };
    let BrowseOpenResponse::Opened { page, .. } = completed else {
        return Err("completed folder query was not available".into());
    };
    assert_eq!(&page.page.result_version, version);
    assert_eq!(page.page.rows.len(), 1);
    assert_eq!(
        page.page.rows[0].display_name,
        BrowseValue::Present {
            value: "replacement.bin".into(),
        }
    );
    let first_page = ipc(
        engine,
        "query.page",
        json!({"queryRef": page.page.query_ref, "resultVersion": version, "start": 0, "count": 1}),
    )?;
    assert!(first_page.error.is_none(), "{:?}", first_page.error);
    let QueryPageResponse::Folder(first_page) = serde_json::from_value(first_page.body)? else {
        return Err("folder result returned a search page".into());
    };
    assert!(first_page.browse_context.is_some());
    assert_eq!(first_page.page.rows, page.page.rows);

    let closed = ipc(
        engine,
        "query.region.close",
        json!({"regionRef": region_ref}),
    )?;
    assert!(closed.error.is_none(), "{:?}", closed.error);
    let error = open(3)?.error.ok_or("closed region was accepted")?;
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.reason_key, "ref.unavailable");
    Ok(())
}
