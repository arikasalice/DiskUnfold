//! Exercises scope revalidation through desktop IPC and the native Windows adapter.
use super::*;
use du_core_contracts::{
    SizeAmount, SizeBasis,
    analysis_ipc::SpaceTotal,
    browse_ipc::BrowseValue,
    browse_page_ipc::{BrowseOpenResponse, QueryPageResponse},
    query_region_ipc::QueryRegionOpened,
};
use rusqlite::Connection;
use std::{fs, io::Write};

const REPLACEMENT_FILES: [(&str, u64); 2] = [("\u{00e9}.bin", 16_384), ("e\u{0301}.bin", 7)];

fn scan_response(
    engine: &Engine,
    draft: &Value,
    review: &ScopeReview,
) -> Result<(String, String, ScanStart), Box<dyn std::error::Error>> {
    let message = json!({"command":"du_call", "args":{"envelope":{
        "name":"scan.start", "contractVersion":1, "correlationId":Id::from_v7_bytes([42; 16]),
        "body":{"draft":draft,"reviewFingerprint":review.review_fingerprint}
    }}})
    .to_string();
    let response = crate::ipc::dispatch(engine, &message).ok_or("missing scan response")?;
    let envelope: ResultEnvelope = serde_json::from_str(&response)?;
    assert!(envelope.error.is_none(), "{:?}", envelope.error);
    Ok((message, response, serde_json::from_value(envelope.body)?))
}

fn wait_for_saved_scope(
    profile: &std::path::Path,
) -> Result<crate::recent_scope_body::RecentScopeBody, Box<dyn std::error::Error>> {
    use rusqlite::OptionalExtension;

    let sql = Connection::open(profile.join(du_platform_os::application_data::DATABASE_FILE))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let (body, format, fingerprint, count): (String, u32, Vec<u8>, u32) = loop {
        let row = sql.query_row(
            "SELECT scope_body, fingerprint_format, scope_fingerprint, include_count FROM user_env_recent_scope",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        if let Some(row) = row {
            break row;
        }
        if Instant::now() >= deadline {
            return Err("recent scope worker did not write".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let rows: u32 = sql.query_row("SELECT count(*) FROM user_env_recent_scope", [], |row| {
        row.get(0)
    })?;
    assert_eq!(rows, 1);
    let fingerprint = fingerprint
        .try_into()
        .map_err(|_| "invalid stored fingerprint width")?;
    crate::recent_scope_body::RecentScopeBody::decode(&body, format, &fingerprint, count)
        .map_err(|e| format!("{e:?}").into())
}

/// Reviews and scans a raw NTFS name whose display requires a replacement character.
#[test]
fn lossy_display_preserves_native_scope_identity() -> Result<(), Box<dyn std::error::Error>> {
    use du_platform_os::webview::ResponseDelivery;
    use std::{ffi::OsString, num::NonZeroUsize, os::windows::ffi::OsStringExt, sync::mpsc};

    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let mut name: Vec<_> = "folder-".encode_utf16().collect();
    name.push(0xd800);
    let target = fixture.path().join(OsString::from_wide(&name));
    fs::create_dir(&target)?;
    fs::write(target.join("item.bin"), [0; 17])?;

    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let services = du_engine_api::desktop::register_services_with_recent_scope_worker(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        &du_platform_os::entropy::OsRandom,
        Arc::new(du_core_contracts::settings::SharedTableLimit::default),
        NonZeroUsize::new(2).ok_or("zero pending capacity")?,
    )?;
    let events = services.events;
    let admissions = services
        .recent_scope_admissions
        .ok_or("missing recent-scope consumer")?;
    let (warn, warnings) = mpsc::channel();
    let worker = crate::recent_scope_worker::RecentScopeWorker::start(
        admissions,
        database,
        NonZeroUsize::new(2).ok_or("zero retention cap")?,
        move |warning| {
            let _ = warn.send(warning);
        },
    )?;
    let observer = worker.observer();
    let engine = builder.build()?;
    let raw: Vec<_> = target
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({
        "includes": [{"encoding": "windows_utf16le", "raw": PathBytes::from_bytes(&raw),
            "display": target.to_string_lossy(), "lossy": true}],
        "exclusions": [], "boundary": "current_volume", "includedMounts": [], "source": "gui"
    });
    let response = ipc(
        &engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    assert!(response.error.is_none(), "{:?}", response.error);
    let review: ScopeReview = serde_json::from_value(response.body)?;
    let [reviewed] = review.scope.includes.as_slice() else {
        return Err("expected one reviewed root".into());
    };
    assert!(reviewed.lossy);
    assert!(reviewed.display.ends_with("folder-\u{fffd}"));
    assert!(reviewed.raw.bytes()?.ends_with(&0xd800_u16.to_le_bytes()));

    let discarded = fixture.path().join("discarded");
    fs::create_dir(&discarded)?;
    let raw: Vec<_> = discarded
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut discarded_draft = draft.clone();
    discarded_draft["includes"][0]["raw"] = json!(PathBytes::from_bytes(&raw));
    let discarded_review: ScopeReview = serde_json::from_value(
        ipc(
            &engine,
            "scope.review",
            json!({"draft": discarded_draft, "requestSeq": 2}),
        )?
        .body,
    )?;
    let (request, response, unposted) =
        scan_response(&engine, &discarded_draft, &discarded_review)?;
    assert!(observer(&request, "{}").is_none());
    let delivery = observer(&request, &response).ok_or("missing response callback")?;
    assert!(observer(&request, &response).is_none());
    delivery(ResponseDelivery::NotPosted);
    terminal_batch(&events, &unposted.job_ref)?;
    let sql = Connection::open(
        profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    let rows: u32 = sql.query_row("SELECT count(*) FROM user_env_recent_scope", [], |row| {
        row.get(0)
    })?;
    assert_eq!(rows, 0);

    sql.execute_batch(
        "CREATE TRIGGER test_recent_fail BEFORE INSERT ON user_env_recent_scope
        BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END;",
    )?;
    let (request, response, failed_write) = scan_response(&engine, &draft, &review)?;
    observer(&request, &response).ok_or("missing failed-write callback")?(ResponseDelivery::Posted);
    assert_eq!(
        warnings.recv_timeout(Duration::from_secs(5))?,
        crate::RecentScopeWriteWarning::StorageUnavailable
    );
    let finished = terminal_batch(&events, &failed_write.job_ref)?;
    assert!(
        finished
            .iter()
            .any(|event| event.topic == "job.finished" && event.body["state"] == "done")
    );
    sql.execute_batch("DROP TRIGGER test_recent_fail")?;

    let (request, response, scan) = scan_response(&engine, &draft, &review)?;
    assert!(!scan.existing);
    let delivery = observer(&request, &response).ok_or("missing response callback")?;
    delivery(ResponseDelivery::Posted);
    let saved = wait_for_saved_scope(profile.path())?;
    assert!(matches!(
        warnings.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(
        saved.scope_draft().includes.as_slice(),
        std::slice::from_ref(reviewed)
    );
    assert!(saved.targets()[0].applied_path.lossy);
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
        json!({"resultVersion": version, "requestSeq": 2, "target": {"kind": "scope_root"}}),
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
            bytes: 17_u64.into()
        }
    );
    Ok(())
}

/// A stale review cannot admit a replacement, even when the pathname is unchanged.
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
    // NTFS can store canonically equivalent names as distinct native entries.
    for (name, bytes) in REPLACEMENT_FILES {
        let mut file = fs::File::create_new(target.join(name))?;
        file.write_all(&vec![2; usize::try_from(bytes)?])?;
    }

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
            bytes: REPLACEMENT_FILES
                .iter()
                .map(|(_, bytes)| bytes)
                .sum::<u64>()
                .into()
        }
    );
    verify_pinned_folder(&engine, &events, version)?;
    Ok(())
}

/// Checks that distinct native names survive scan, indexing and pinned paging.
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
    assert_eq!(page.page.rows.len(), REPLACEMENT_FILES.len());
    assert_ne!(page.page.rows[0].item, page.page.rows[1].item);
    for (name, bytes) in REPLACEMENT_FILES {
        let row = page
            .page
            .rows
            .iter()
            .find(
                |row| matches!(&row.display_name, BrowseValue::Present { value } if value == name),
            )
            .ok_or_else(|| format!("missing native name {name:?}"))?;
        assert_eq!(&row.item.result_version, version);
        let BrowseValue::Present { value } = &row.logical_size else {
            return Err(format!("missing logical size for {name:?}").into());
        };
        assert_eq!(
            value.amount,
            SizeAmount::Exact {
                bytes: bytes.into()
            }
        );
    }
    let first_page = ipc(
        engine,
        "query.page",
        json!({"queryRef": page.page.query_ref, "resultVersion": version, "start": 0, "count": REPLACEMENT_FILES.len()}),
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
