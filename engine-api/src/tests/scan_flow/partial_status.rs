use super::*;
use du_core_contracts::result::ResultCompleteness;

#[test]
fn status_exposes_partial_for_its_admitted_job_and_not_previous_scan()
-> Result<(), Box<dyn std::error::Error>> {
    let (finish, gate) = mpsc::channel();
    let harness = scan_harness_with_gate(Some(Arc::new(Mutex::new(gate))))?;
    let gui = |name, body| call(&harness.engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let raw: Vec<u8> = "C:\\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({"includes": [{"encoding":"windows_utf16le", "raw":PathBytes::from_bytes(&raw), "display":"", "lossy":false}], "exclusions":[], "boundary":"current_volume", "includedMounts":[], "source":"gui"});
    let review: ScopeReview =
        serde_json::from_value(gui("scope.review", json!({"draft":draft, "requestSeq":1}))?.body)?;
    let start = || -> Result<ScanStart, Box<dyn std::error::Error>> {
        Ok(serde_json::from_value(
            gui(
                "scan.start",
                json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
            )?
            .body,
        )?)
    };
    let status = |job| read_status(&harness.engine, job);
    let first = start()?;
    let duplicate = start()?;
    assert_eq!(first.lineage_id, duplicate.lineage_id);
    assert_eq!(first.job_ref, duplicate.job_ref);
    let early = status(&first.job_ref)?;
    assert_eq!(early["scanResult"]["lineageId"], json!(first.lineage_id));
    assert_eq!(early["scanResult"]["sequence"], 0);
    assert!(early["scanResult"].get("resultVersion").is_none());
    harness.release.send(())?;
    let notice = match harness.terminal.recv_timeout(Duration::from_secs(5))? {
        JobEvent::ResultVersionChanged(notice) => notice,
        _ => return Err("missing partial event".into()),
    };
    let pinned = harness
        .versions
        .pin_current(notice.result_version.as_ref().ok_or("missing partial ID")?)
        .map_err(|error| format!("{error:?}"))?;
    let partial = status(&first.job_ref)?;
    assert_eq!(partial["state"], "running");
    assert_eq!(pinned.completeness(), ResultCompleteness::Partial);
    assert_eq!(
        partial["scanResult"]["resultVersion"],
        json!(notice.result_version)
    );
    assert_eq!(
        partial["scanResult"]["sequence"],
        json!(notice.body.sequence)
    );
    assert!(partial["resultVersion"].is_null());
    finish.send(())?;
    assert_finalized_partial(&harness, &first.job_ref, &pinned)?;
    let second = start()?;
    assert_eq!(first.lineage_id, second.lineage_id);
    assert_ne!(first.scope_version, second.scope_version);
    let before = status(&second.job_ref)?;
    assert!(before["scanResult"].get("resultVersion").is_none());
    assert!(before["scanResult"]["sequence"].as_u64().unwrap_or(0) >= 2);
    harness.release.send(())?;
    let next = match harness.terminal.recv_timeout(Duration::from_secs(5))? {
        JobEvent::ResultVersionChanged(notice) => notice,
        _ => return Err("missing second partial event".into()),
    };
    let second_partial = status(&second.job_ref)?;
    assert_eq!(
        second_partial["scanResult"]["resultVersion"],
        json!(next.result_version)
    );
    assert!(
        status(&first.job_ref)?["scanResult"]
            .get("resultVersion")
            .is_none()
    );
    finish.send(())?;
    let _ = scan_finished(&harness.terminal)?;
    Ok(())
}

fn read_status(engine: &Engine, job: &Id) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(call(
        engine,
        "job.status",
        Channel::Gui,
        ENVELOPE_VERSION,
        json!({"jobRef": job}),
    )?
    .body)
}

fn assert_finalized_partial(
    harness: &ScanHarness,
    job: &Id,
    pinned: &du_analysis_index::ResultVersion,
) -> Result<(), Box<dyn std::error::Error>> {
    let (summary, _) = scan_finished(&harness.terminal)?;
    let final_id = summary.result_version_id.ok_or("missing first final")?;
    let done = read_status(&harness.engine, job)?;
    assert_eq!(done["state"], "done");
    assert_eq!(pinned.completeness(), ResultCompleteness::Partial);
    assert_ne!(pinned.version_id(), &final_id);
    assert_eq!(done["resultVersion"], json!(final_id));
    assert_eq!(done["scanResult"]["resultVersion"], json!(final_id));
    Ok(())
}

#[test]
fn other_bound_job_kinds_do_not_expose_scan_result() -> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::job::{JobBody, JobContext, JobCounts, JobOutcome};
    use du_job_runtime::{JobPriority, ResultBinding, Submission, SubmitResult};
    struct Other;
    impl JobBody for Other {
        fn kind_id(&self) -> &str {
            "refresh.other"
        }
        fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
            JobOutcome {
                counts: JobCounts::default(),
                completed_scope: true,
                failure_reason_key: None,
                unprocessed_summary: None,
                result_version_id: None,
            }
        }
    }
    let ids = Arc::new(Ids {
        next: AtomicUsize::new(1),
        fallback: Id::parse("018f0000-0000-7000-8000-000000000000")?,
    });
    let (tx, _) = mpsc::channel();
    let runtime = Arc::new(JobRuntime::new(
        ids,
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(Events(tx)),
    )?);
    let mut descriptor = du_scan_engine::scan_job::descriptor();
    descriptor.kind_id = "refresh.other".into();
    runtime.register(descriptor)?;
    let lineage = Id::parse("018f0000-0000-7000-8000-000000000010")?;
    let scope = Id::parse("018f0000-0000-7000-8000-000000000011")?;
    let submitted = runtime.submit_with_result_binding(
        Submission {
            kind_id: "refresh.other".into(),
            deduplication_key: None,
            body: Arc::new(Other),
            priority: JobPriority::Normal,
            scope_summary: "other".into(),
            correlation_id: scope.clone(),
            sample: false,
        },
        ResultBinding {
            lineage_id: lineage,
            scope_version: scope,
        },
    )?;
    let job = match submitted {
        SubmitResult::Created(id) => id,
        _ => return Err("unexpected reuse".into()),
    };
    let mut builder = EngineBuilder::default();
    crate::jobs::register_with_scan_store(
        &mut builder,
        runtime,
        du_analysis_index::IngestSink::default(),
    );
    let engine = builder.build()?;
    let status = call(
        &engine,
        "job.status",
        Channel::Gui,
        ENVELOPE_VERSION,
        json!({"jobRef":job}),
    )?;
    assert!(status.body.get("scanResult").is_none());
    Ok(())
}
