use super::*;

#[test]
fn scan_start_resource_limit_boundaries_follow_the_largest_lane_permit()
-> Result<(), Box<dyn std::error::Error>> {
    let hdd = scan_harness()?;
    set_device(&hdd, b'P', &[2], device(DeviceKind::Hdd, Some(b"hdd-disk")))?;
    let one_root = draft();
    let review_hdd = review(&hdd, &one_root)?;
    let accepted: ScanStart = serde_json::from_value(
        start(
            &hdd,
            &one_root,
            &review_hdd.review_fingerprint,
            Some(json!({ "userLimit": 4 })),
        )?
        .body,
    )?;
    assert_eq!(
        hdd.roots
            .device_probes
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    let rejected_update = update_resource(
        &hdd,
        Channel::Gui,
        json!({
            "jobRef": accepted.job_ref,
            "resourceMode": "fast",
            "userLimit": 5
        }),
    )?;
    assert_eq!(
        rejected_update
            .error
            .ok_or("set_resource accepted HDD limit 5")?
            .reason_key,
        "scan.resource_limit_invalid"
    );
    assert_eq!(
        hdd.roots
            .device_probes
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    complete_start(&hdd, accepted, 1)?;

    let hdd_invalid = scan_harness()?;
    set_device(
        &hdd_invalid,
        b'P',
        &[2],
        device(DeviceKind::Hdd, Some(b"hdd-disk")),
    )?;
    let review_hdd_invalid = review(&hdd_invalid, &one_root)?;
    let rejected = start(
        &hdd_invalid,
        &one_root,
        &review_hdd_invalid.review_fingerprint,
        Some(json!({ "userLimit": 5 })),
    )?;
    assert_eq!(
        rejected.error.ok_or("HDD limit 5 was accepted")?.reason_key,
        "scan.resource_limit_invalid"
    );
    assert_no_jobs(&hdd_invalid)?;

    let mixed = scan_harness()?;
    set_device(
        &mixed,
        b'P',
        &[2],
        device(DeviceKind::Ssd, Some(b"ssd-disk")),
    )?;
    set_device(
        &mixed,
        b'Q',
        &[3],
        device(DeviceKind::Hdd, Some(b"hdd-disk")),
    )?;
    let two_roots = draft_for(&["P:\\PriorityFixture", "Q:\\SecondFixture"]);
    let review_mixed = review(&mixed, &two_roots)?;
    let accepted: ScanStart = serde_json::from_value(
        start(
            &mixed,
            &two_roots,
            &review_mixed.review_fingerprint,
            Some(json!({ "userLimit": 64 })),
        )?
        .body,
    )?;
    complete_start(&mixed, accepted, 2)?;

    let mixed_invalid = scan_harness()?;
    set_device(
        &mixed_invalid,
        b'P',
        &[2],
        device(DeviceKind::Ssd, Some(b"ssd-disk")),
    )?;
    set_device(
        &mixed_invalid,
        b'Q',
        &[3],
        device(DeviceKind::Hdd, Some(b"hdd-disk")),
    )?;
    let review_mixed_invalid = review(&mixed_invalid, &two_roots)?;
    let rejected = start(
        &mixed_invalid,
        &two_roots,
        &review_mixed_invalid.review_fingerprint,
        Some(json!({ "userLimit": 65 })),
    )?;
    assert_eq!(
        rejected
            .error
            .ok_or("mixed SSD limit 65 was accepted")?
            .reason_key,
        "scan.resource_limit_invalid"
    );
    assert_no_jobs(&mixed_invalid)?;

    let unknown = scan_harness()?;
    let review_unknown = review(&unknown, &one_root)?;
    let accepted: ScanStart = serde_json::from_value(
        start(
            &unknown,
            &one_root,
            &review_unknown.review_fingerprint,
            Some(json!({ "userLimit": 16 })),
        )?
        .body,
    )?;
    complete_start(&unknown, accepted, 1)?;

    let unknown_invalid = scan_harness()?;
    let review_unknown_invalid = review(&unknown_invalid, &one_root)?;
    let rejected = start(
        &unknown_invalid,
        &one_root,
        &review_unknown_invalid.review_fingerprint,
        Some(json!({ "userLimit": 17 })),
    )?;
    assert_eq!(
        rejected
            .error
            .ok_or("unknown-only limit 17 was accepted")?
            .reason_key,
        "scan.resource_limit_invalid"
    );
    assert_no_jobs(&unknown_invalid)?;
    Ok(())
}

#[test]
fn query_failed_device_remains_unknown_and_scan_completes() -> TestResult {
    let harness = scan_harness()?;
    set_device(
        &harness,
        b'P',
        &[2],
        VolumeDevice {
            kind: DeviceKind::Unknown,
            kind_capability: Capability {
                state: CapabilityState::Partial,
                reason_key: Some("os.query_failed"),
            },
            disk_key: None,
            disk_capability: Capability {
                state: CapabilityState::Unsupported,
                reason_key: Some("os.unsupported"),
            },
        },
    )?;
    let one_root = draft();
    let reviewed = review(&harness, &one_root)?;
    let receipt: ScanStart = serde_json::from_value(
        start(
            &harness,
            &one_root,
            &reviewed.review_fingerprint,
            Some(json!({ "userLimit": 16 })),
        )?
        .body,
    )?;
    complete_start(&harness, receipt, 1)?;
    let status = harness
        .lane_status
        .try_iter()
        .last()
        .ok_or("lane status was not reported")?;
    let lane = status.status.lanes.first().ok_or("lane status was empty")?;
    assert_eq!(lane.device_kind, DeviceKind::Unknown);
    assert_eq!(lane.device_kind_source, DeviceKindSource::Unknown);
    assert_eq!(lane.permit, Some(16));
    Ok(())
}
