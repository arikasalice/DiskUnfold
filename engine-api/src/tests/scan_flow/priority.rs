use super::*;

struct PriorityRun {
    harness: ScanHarness,
    started: ScanStart,
    area_a: ItemRef,
    area_b: ItemRef,
    file: ItemRef,
}

#[test]
fn scan_priority_uses_the_owned_job_and_its_current_partial_item()
-> Result<(), Box<dyn std::error::Error>> {
    let run = start_priority_run()?;
    assert_invalid_priority_requests(&run)?;
    assert_priority_replacement_and_completion(&run)?;
    assert_terminal_priority(&run)?;
    Ok(())
}

fn start_priority_run() -> Result<PriorityRun, Box<dyn std::error::Error>> {
    let harness = fixture::scan_harness_with_priority()?;
    let priority_opened = harness
        .priority_opened
        .as_ref()
        .ok_or("missing priority open gate")?;
    let path: Vec<u8> = "P:\\PriorityFixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({
        "includes": [{
            "encoding": "windows_utf16le",
            "raw": PathBytes::from_bytes(&path),
            "display": "P:\\PriorityFixture",
            "lossy": false
        }],
        "exclusions": [],
        "boundary": "current_volume",
        "includedMounts": [],
        "source": "gui"
    });
    let review = call(
        &harness.engine,
        "scope.review",
        Channel::Gui,
        ENVELOPE_VERSION,
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    assert!(review.error.is_none());
    let review: ScopeReview = serde_json::from_value(review.body)?;
    let started = call(
        &harness.engine,
        "scan.start",
        Channel::Gui,
        ENVELOPE_VERSION,
        json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
    )?;
    assert!(started.error.is_none());
    let started: ScanStart = serde_json::from_value(started.body)?;
    priority_opened.recv_timeout(Duration::from_secs(5))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let pinned = harness
            .versions
            .pin()
            .map_err(|error| format!("pin scan result: {error:?}"))?;
        if let Some(version) = pinned.filter(|version| {
            version.scope_version_id() == &started.scope_version
                && version.lineage_id() == started.lineage_id.as_ref()
                && version.completeness() == du_core_contracts::result::ResultCompleteness::Partial
        }) {
            let reference = |name: &[u8], kind| {
                version
                    .items()
                    .iter()
                    .find(|item| item.kind == kind && item.name == name)
                    .map(|item| ItemRef {
                        result_version: version.version_id().clone(),
                        item_no: item.item_no,
                    })
            };
            if let (Some(area_a), Some(area_b), Some(file)) = (
                reference(b"A\0", du_core_contracts::result::ResultItemKind::Directory),
                reference(b"B\0", du_core_contracts::result::ResultItemKind::Directory),
                reference(b"F\0", du_core_contracts::result::ResultItemKind::File),
            ) {
                return Ok(PriorityRun {
                    harness,
                    started,
                    area_a,
                    area_b,
                    file,
                });
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err("partial scan result did not publish A, B, and file F".into());
        }
        std::thread::yield_now();
    }
}

fn prioritize(
    run: &PriorityRun,
    area: &ItemRef,
    job_ref: &Id,
    channel: Channel,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    prioritize_body(
        run,
        json!({"jobRef": job_ref, "priorityArea": area}),
        channel,
    )
}

fn prioritize_body(
    run: &PriorityRun,
    body: serde_json::Value,
    channel: Channel,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    Ok(call(
        &run.harness.engine,
        "scan.prioritize",
        channel,
        ENVELOPE_VERSION,
        body,
    )?)
}

/// Publishes two results in an unrelated lineage of the same store and
/// returns a folder from the superseded and from the current result.
fn other_lineage_areas(
    versions: &du_analysis_index::IngestSink,
) -> Result<(ItemRef, ItemRef), Box<dyn std::error::Error>> {
    use du_core_contracts::result::{
        ResultCompleteness, ResultItemKind, ResultNativePath, ResultScopeRoot,
    };
    let lineage = versions
        .bind_current_analysis("priority-other-lineage", Id::from_v7_bytes([0x51; 16]))
        .map_err(|error| format!("bind other lineage: {error:?}"))?;
    let publish = |scope: u8, version: u8| -> Result<ItemRef, Box<dyn std::error::Error>> {
        let root = ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(&[b'Q', 0]),
            },
            volume_id: vec![1],
            file_id: vec![scope],
        };
        let mut build = versions
            .open_forest_in_lineage(Id::from_v7_bytes([scope; 16]), vec![root], lineage.clone())
            .map_err(|error| format!("open other lineage: {error:?}"))?;
        build
            .add_directory_at(0, Vec::new())
            .map_err(|error| format!("add other root: {error:?}"))?;
        let version = build
            .seal(
                Id::from_v7_bytes([version; 16]),
                ResultCompleteness::Partial,
            )
            .map_err(|error| format!("publish other lineage: {error:?}"))?;
        let item = version
            .items()
            .iter()
            .find(|item| item.kind == ResultItemKind::Directory)
            .ok_or("missing other lineage folder")?;
        Ok(ItemRef {
            result_version: version.version_id().clone(),
            item_no: item.item_no,
        })
    };
    let superseded = publish(0x52, 0x53)?;
    let current = publish(0x54, 0x55)?;
    Ok((superseded, current))
}

fn assert_invalid_priority_requests(run: &PriorityRun) -> Result<(), Box<dyn std::error::Error>> {
    let (superseded, other_lineage) = other_lineage_areas(&run.harness.versions)?;
    let opens = run.harness.roots.opens.load(Ordering::Acquire);
    let reads = run.harness.roots.reads.load(Ordering::Acquire);
    let job = &run.started.job_ref;
    let area = &run.area_a;
    let missing_item = ItemRef {
        result_version: area.result_version.clone(),
        item_no: 9_999,
    };
    let random_version = ItemRef {
        result_version: Id::from_v7_bytes([8; 16]),
        item_no: area.item_no,
    };
    let cases = [
        (
            json!({"jobRef": Id::from_v7_bytes([9; 16]), "priorityArea": area}),
            Channel::Gui,
            "job.not_found",
        ),
        (
            json!({"jobRef": job, "priorityArea": random_version}),
            Channel::Gui,
            "result.version_stale",
        ),
        (
            json!({"jobRef": job, "priorityArea": superseded}),
            Channel::Gui,
            "result.version_stale",
        ),
        (
            json!({"jobRef": job, "priorityArea": other_lineage}),
            Channel::Gui,
            "scan.priority_out_of_scope",
        ),
        (
            json!({"jobRef": job, "priorityArea": run.file}),
            Channel::Gui,
            "scan.priority_out_of_scope",
        ),
        (
            json!({"jobRef": job, "priorityArea": missing_item}),
            Channel::Gui,
            "scan.priority_out_of_scope",
        ),
        (
            json!({"jobRef": job, "priorityArea": area, "path": "C:/untrusted"}),
            Channel::Gui,
            "command.body_invalid",
        ),
        (
            json!({"jobRef": 7, "priorityArea": area}),
            Channel::Gui,
            "command.body_invalid",
        ),
        (
            json!({"jobRef": job, "priorityArea": area}),
            Channel::Cli,
            "command.not_provided",
        ),
        (
            json!({"jobRef": job, "priorityArea": area}),
            Channel::Mcp,
            "command.not_provided",
        ),
    ];
    for (body, channel, reason) in cases {
        let reply = prioritize_body(run, body.clone(), channel)?;
        let error = reply
            .error
            .ok_or_else(|| format!("invalid priority request was accepted: {body}"))?;
        assert_eq!(error.reason_key, reason, "{body}");
        if reason == "scan.priority_out_of_scope" || reason == "command.body_invalid" {
            assert_eq!(error.code, du_core_contracts::ErrorCode::InvalidArgument);
        }
    }
    // Rejected requests neither query the file system nor reach the control
    // handle: the first valid request for area A is still a new signal.
    assert_eq!(run.harness.roots.opens.load(Ordering::Acquire), opens);
    assert_eq!(run.harness.roots.reads.load(Ordering::Acquire), reads);
    Ok(())
}

fn assert_priority_replacement_and_completion(
    run: &PriorityRun,
) -> Result<(), Box<dyn std::error::Error>> {
    let first = prioritize(run, &run.area_a, &run.started.job_ref, Channel::Gui)?;
    assert!(first.error.is_none());
    assert_eq!(first.body["controlResult"], "accepted");
    assert_eq!(
        first.body["appliedPriorityArea"]["itemNo"],
        run.area_a.item_no
    );
    let current = run
        .harness
        .versions
        .pin_current(&run.area_a.result_version)
        .map_err(|error| format!("pin accepted priority result: {error:?}"))?;
    assert_eq!(current.version_id(), &run.area_a.result_version);

    let replacement = prioritize(run, &run.area_b, &run.started.job_ref, Channel::Gui)?;
    assert!(replacement.error.is_none());
    assert_eq!(replacement.body["controlResult"], "accepted");
    assert_eq!(
        replacement.body["appliedPriorityArea"]["itemNo"],
        run.area_b.item_no
    );

    let repeated = prioritize(run, &run.area_b, &run.started.job_ref, Channel::Gui)?;
    assert!(repeated.error.is_none());
    assert_eq!(repeated.body["controlResult"], "no_effect");
    assert_eq!(
        repeated.body["appliedPriorityArea"]["itemNo"],
        run.area_b.item_no
    );

    run.harness.release.send(())?;
    assert_eq!(
        run.harness
            .priority_visits
            .as_ref()
            .ok_or("missing priority visit observer")?
            .recv_timeout(Duration::from_secs(5))?,
        "B"
    );
    run.harness
        .priority_visit_entered
        .as_ref()
        .ok_or("missing visit gate")?
        .recv_timeout(Duration::from_secs(5))?;
    run.harness
        .priority_b_completed
        .as_ref()
        .ok_or("missing B completion signal")?
        .recv_timeout(Duration::from_secs(5))?;
    let completed_area = prioritize(run, &run.area_b, &run.started.job_ref, Channel::Gui)?;
    assert!(completed_area.error.is_none());
    assert_eq!(completed_area.body["controlResult"], "no_effect");
    assert!(completed_area.body.get("appliedPriorityArea").is_none());
    run.harness
        .priority_visit_release
        .as_ref()
        .ok_or("missing visit release sender")?
        .send(())?;
    assert_eq!(
        run.harness
            .priority_visits
            .as_ref()
            .ok_or("missing priority visit observer")?
            .recv_timeout(Duration::from_secs(5))?,
        "A"
    );
    Ok(())
}

fn assert_terminal_priority(run: &PriorityRun) -> Result<(), Box<dyn std::error::Error>> {
    let delivery = scan_delivery(&run.harness.terminal)?;
    let final_version = check_terminal_scan(
        &run.harness.engine,
        *delivery.summary,
        delivery.history_recorded,
        &run.started.job_ref,
    )?;
    assert_ne!(final_version, run.area_a.result_version);
    let terminal = prioritize(run, &run.area_b, &run.started.job_ref, Channel::Gui)?;
    assert!(terminal.error.is_none());
    assert_eq!(terminal.body["controlResult"], "already_terminal");
    assert!(terminal.body.get("appliedPriorityArea").is_none());
    Ok(())
}
