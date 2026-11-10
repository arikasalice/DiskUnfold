//! Native Windows scan-to-browse completion through the desktop IPC dispatcher.
use super::*;
use du_core_contracts::{
    SizeAmount, SizeBasis,
    analysis_ipc::{AnalysisJobStatus, SpaceTotal},
    browse_page_ipc::{BrowseOpenResponse, QueryPageResponse},
    query_region_ipc::QueryRegionOpened,
};
use std::fs;

#[test]
fn scanned_scope_async_browse_recovers_query_from_job_get_and_finished()
-> Result<(), Box<dyn std::error::Error>> {
    const FILES: usize = 2_100;
    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    for index in 0..FILES {
        fs::write(fixture.path().join(format!("entry-{index:05}.bin")), [1])?;
    }
    let database = crate::startup_database::open(profile.path())?;
    let random = &du_platform_os::entropy::OsRandom;
    let mut builder = EngineBuilder::default();
    let events = du_engine_api::desktop::register(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        random,
    )?;
    let engine = builder.build()?;
    let raw: Vec<_> = fixture
        .path()
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({"includes":[{"encoding":"windows_utf16le", "raw":PathBytes::from_bytes(&raw),
        "display":fixture.path(), "lossy":false}], "exclusions":[], "boundary":"current_volume",
        "includedMounts":[], "source":"gui"});
    let review = ipc(
        &engine,
        "scope.review",
        json!({"draft":draft,"requestSeq":1}),
    )?;
    assert!(review.error.is_none(), "{:?}", review.error);
    let review: ScopeReview = serde_json::from_value(review.body)?;
    let start = ipc(
        &engine,
        "scan.start",
        json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
    )?;
    assert!(start.error.is_none(), "{:?}", start.error);
    let scan: ScanStart = serde_json::from_value(start.body)?;
    let batch = terminal_batch(&events, &scan.job_ref)?;
    let scan_finished = batch
        .iter()
        .find(|event| {
            event.topic == "job.finished" && event.job_ref.as_ref() == Some(&scan.job_ref)
        })
        .ok_or("missing scan completion")?;
    assert_eq!(scan_finished.body["state"], "done");
    assert!(scan_finished.body.get("queryRef").is_none());
    let version = scan_finished
        .result_version
        .clone()
        .ok_or("missing scan result version")?;
    let scan_status = ipc(&engine, "job.get", json!({"jobRef":scan.job_ref}))?;
    assert!(scan_status.error.is_none(), "{:?}", scan_status.error);
    let scan_status: AnalysisJobStatus = serde_json::from_value(scan_status.body)?;
    assert_eq!(scan_status.result_version.as_ref(), Some(&version));
    assert!(scan_status.query_ref.is_none());
    let total = ipc(
        &engine,
        "space.total",
        json!({"resultVersion":version,"requestSeq":2,"target":{"kind":"scope_root"}}),
    )?;
    assert!(total.error.is_none(), "{:?}", total.error);
    let total: SpaceTotal = serde_json::from_value(total.body)?;
    assert_eq!(total.result_version, version);
    assert_eq!(
        total
            .sizes
            .iter()
            .find(|value| value.size.basis == SizeBasis::Logical)
            .ok_or("missing logical total")?
            .size
            .amount,
        SizeAmount::Exact {
            bytes: (FILES as u64).into()
        }
    );
    verify_async_browse(&engine, &events, version, scan.job_ref)
}

fn verify_async_browse(
    engine: &Engine,
    events: &du_engine_api::desktop::DesktopEvents,
    version: Id,
    scan_job: Id,
) -> Result<(), Box<dyn std::error::Error>> {
    let issued = ipc(engine, "query.region.open", json!({"kind":"browse_list"}))?;
    assert!(issued.error.is_none(), "{:?}", issued.error);
    let QueryRegionOpened { region_ref } = serde_json::from_value(issued.body)?;
    // The source version is intentionally omitted: admission must return its pinned value.
    let opened = ipc(
        engine,
        "browse.open",
        json!({"regionRef":region_ref,"viewKind":"list","requestSeq":1,"pageSize":200}),
    )?;
    assert!(opened.error.is_none(), "{:?}", opened.error);
    let BrowseOpenResponse::Job {
        job_ref,
        result_version,
        ..
    } = serde_json::from_value(opened.body)?
    else {
        return Err("fixture did not take the asynchronous browse path".into());
    };
    assert_eq!(result_version, version);
    let batch = terminal_batch(events, &job_ref)?;
    let finished = batch
        .iter()
        .find(|event| event.topic == "job.finished" && event.job_ref.as_ref() == Some(&job_ref))
        .ok_or("missing query completion")?;
    assert_eq!(finished.body["state"], "done");
    assert!(finished.result_version.is_none());
    let query_ref: Id = serde_json::from_value(finished.body["queryRef"].clone())?;
    assert_ne!(query_ref, version);
    assert_ne!(job_ref, scan_job);
    let status = ipc(engine, "job.get", json!({"jobRef":job_ref}))?;
    assert!(status.error.is_none(), "{:?}", status.error);
    let status: AnalysisJobStatus = serde_json::from_value(status.body)?;
    assert_eq!(status.job_ref, job_ref);
    assert_eq!(status.kind, "query.evaluate");
    assert_eq!(status.state, JobState::Done);
    assert_eq!(finished.body["sequence"], json!(status.sequence));
    assert_eq!(status.query_ref.as_ref(), Some(&query_ref));
    assert!(status.result_version.is_none());
    let page = ipc(
        engine,
        "query.page",
        json!({"queryRef":query_ref,"resultVersion":result_version,"start":0,"count":200}),
    )?;
    assert!(page.error.is_none(), "{:?}", page.error);
    let QueryPageResponse::Folder(page) = serde_json::from_value(page.body)? else {
        return Err("expected folder page".into());
    };
    assert_eq!(page.page.query_ref, query_ref);
    assert_eq!(page.page.result_version, version);
    assert_eq!(page.page.rows.len(), 200);
    assert!(page.browse_context.is_some());
    let wrong_version = Id::from_v7_bytes([11; 16]);
    let rejected = ipc(
        engine,
        "query.page",
        json!({"queryRef":query_ref,"resultVersion":wrong_version,"start":0,"count":1}),
    )?;
    let error = rejected.error.ok_or("different version was accepted")?;
    assert_eq!(error.code, ErrorCode::StaleVersion);
    assert_eq!(error.reason_key, "result.requery_required");
    Ok(())
}
