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
    Ok(call(
        &run.harness.engine,
        "scan.prioritize",
        channel,
        ENVELOPE_VERSION,
        json!({"jobRef": job_ref, "priorityArea": area}),
    )?)
}

fn assert_invalid_priority_requests(run: &PriorityRun) -> Result<(), Box<dyn std::error::Error>> {
    let unknown = prioritize(run, &run.area_a, &Id::from_v7_bytes([9; 16]), Channel::Gui)?;
    assert_eq!(
        unknown.error.ok_or("unknown job was accepted")?.reason_key,
        "job.not_found"
    );
    let stale = ItemRef {
        result_version: Id::from_v7_bytes([8; 16]),
        item_no: run.area_b.item_no,
    };
    assert_eq!(
        prioritize(run, &stale, &run.started.job_ref, Channel::Gui)?
            .error
            .ok_or("stale result version was accepted")?
            .reason_key,
        "result.version_stale"
    );
    assert_eq!(
        prioritize(run, &run.file, &run.started.job_ref, Channel::Gui)?
            .error
            .ok_or("file priority area was accepted")?
            .reason_key,
        "scan.priority_out_of_scope"
    );
    assert_eq!(
        prioritize(run, &run.area_b, &run.started.job_ref, Channel::Cli)?
            .error
            .ok_or("non-GUI priority call was accepted")?
            .reason_key,
        "command.not_provided"
    );
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
