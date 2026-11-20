use super::*;
use du_core_contracts::analysis_ipc::{ScanCollection, ScanStart, ScopeReview};
use du_core_contracts::{
    scan_observation::{DeviceKind, DeviceKindSource, VolumeDevice},
    volume::{Capability, CapabilityState},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn draft() -> serde_json::Value {
    let path: Vec<u8> = "P:\\PriorityFixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    json!({
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
    })
}

fn draft_for(paths: &[&str]) -> serde_json::Value {
    let includes: Vec<_> = paths
        .iter()
        .map(|path| {
            let raw: Vec<u8> = path.encode_utf16().flat_map(u16::to_le_bytes).collect();
            json!({
                "encoding": "windows_utf16le",
                "raw": PathBytes::from_bytes(&raw),
                "display": path,
                "lossy": false
            })
        })
        .collect();
    json!({
        "includes": includes,
        "exclusions": [],
        "boundary": "current_volume",
        "includedMounts": [],
        "source": "gui"
    })
}

fn device(kind: DeviceKind, disk_key: Option<&[u8]>) -> VolumeDevice {
    VolumeDevice {
        kind,
        kind_capability: Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        },
        disk_key: disk_key.map(<[u8]>::to_vec),
        disk_capability: Capability {
            state: if disk_key.is_some() {
                CapabilityState::Supported
            } else {
                CapabilityState::Unsupported
            },
            reason_key: None,
        },
    }
}

fn set_device(
    harness: &ScanHarness,
    prefix: u8,
    volume: &[u8],
    device: VolumeDevice,
) -> Result<(), Box<dyn std::error::Error>> {
    harness
        .roots
        .device_overrides
        .lock()
        .map_err(|_| "device override lock poisoned")?
        .insert(prefix, (volume.to_vec(), device));
    Ok(())
}

fn assert_no_jobs(harness: &ScanHarness) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        call(
            &harness.engine,
            "job.list",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({})
        )?
        .body,
        json!([])
    );
    Ok(())
}

fn complete_start(harness: &ScanHarness, receipt: ScanStart, release_count: usize) -> TestResult {
    for _ in 0..release_count {
        harness.release.send(())?;
    }
    let (summary, _) = scan_finished(&harness.terminal)?;
    assert_eq!(summary.job_id, receipt.job_ref);
    Ok(())
}

fn review(
    harness: &ScanHarness,
    draft: &serde_json::Value,
) -> Result<ScopeReview, Box<dyn std::error::Error>> {
    let response = call(
        &harness.engine,
        "scope.review",
        Channel::Gui,
        ENVELOPE_VERSION,
        json!({ "draft": draft, "requestSeq": 1 }),
    )?;
    if let Some(error) = response.error {
        return Err(format!("scope review failed: {}", error.reason_key).into());
    }
    Ok(serde_json::from_value(response.body)?)
}

fn start(
    harness: &ScanHarness,
    draft: &serde_json::Value,
    fingerprint: &str,
    collection: Option<serde_json::Value>,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    let mut body = json!({ "draft": draft, "reviewFingerprint": fingerprint });
    if let Some(collection) = collection {
        body["collection"] = collection;
    }
    Ok(call(
        &harness.engine,
        "scan.start",
        Channel::Gui,
        ENVELOPE_VERSION,
        body,
    )?)
}

fn update_resource(
    harness: &ScanHarness,
    channel: Channel,
    body: serde_json::Value,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    Ok(call(
        &harness.engine,
        "scan.set_resource",
        channel,
        ENVELOPE_VERSION,
        body,
    )?)
}

#[test]
fn scan_start_validates_before_job_registration_and_duplicates_keep_first_condition()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = fixture::scan_harness_with_priority()?;
    let draft = draft();
    let review = review(&harness, &draft)?;
    for user_limit in [0, 17] {
        let reply = start(
            &harness,
            &draft,
            &review.review_fingerprint,
            Some(json!({ "userLimit": user_limit })),
        )?;
        assert_eq!(
            reply
                .error
                .ok_or("invalid resource limit was accepted")?
                .reason_key,
            "scan.resource_limit_invalid"
        );
    }
    assert_eq!(
        call(
            &harness.engine,
            "job.list",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({})
        )?
        .body,
        json!([]),
        "invalid starts must not register a job"
    );

    let receipt: ScanStart =
        serde_json::from_value(start(&harness, &draft, &review.review_fingerprint, None)?.body)?;
    assert_eq!(
        receipt.collection,
        Some(ScanCollection {
            resource_mode: du_core_contracts::analysis_ipc::ScanResourceMode::Auto,
            user_limit: None,
        })
    );
    harness
        .priority_opened
        .as_ref()
        .ok_or("missing scan open gate")?
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| {
            format!(
                "priority child open did not start ({error:?}); root opens {}, reads {}",
                harness.roots.opens.load(Ordering::Acquire),
                harness.roots.reads.load(Ordering::Acquire)
            )
        })?;

    let duplicate: ScanStart = serde_json::from_value(
        start(
            &harness,
            &draft,
            &review.review_fingerprint,
            Some(json!({ "resourceMode": "fast", "userLimit": 9 })),
        )?
        .body,
    )?;
    assert!(duplicate.existing);
    assert_eq!(duplicate.job_ref, receipt.job_ref);
    assert_eq!(duplicate.collection, receipt.collection);
    if let Some(release) = &harness.priority_visit_release {
        release.send(())?;
    }
    harness.release.send(())?;
    let (summary, _) = scan_finished(&harness.terminal)?;
    assert_eq!(summary.job_id, receipt.job_ref);
    Ok(())
}

fn assert_resource_updates(
    harness: &ScanHarness,
    draft: &serde_json::Value,
    fingerprint: &str,
    job_ref: &Id,
) -> Result<(), Box<dyn std::error::Error>> {
    let repeated = update_resource(
        harness,
        Channel::Gui,
        json!({ "jobRef": job_ref, "resourceMode": "quiet", "userLimit": 2 }),
    )?;
    assert!(repeated.error.is_none());
    assert_eq!(repeated.body["outcome"], "no_effect");
    assert_eq!(repeated.body["lanes"][0]["effectiveLimit"], 2);
    assert_eq!(repeated.body["lanes"][0]["permit"], 16);
    assert_eq!(repeated.body["appliesFrom"], "next_assignment");

    for user_limit in [0, 17] {
        let invalid = update_resource(
            harness,
            Channel::Gui,
            json!({
                "jobRef": job_ref,
                "resourceMode": "fast",
                "userLimit": user_limit
            }),
        )?;
        assert_eq!(
            invalid
                .error
                .ok_or("invalid limit was accepted")?
                .reason_key,
            "scan.resource_limit_invalid"
        );
    }

    let raised = update_resource(
        harness,
        Channel::Gui,
        json!({ "jobRef": job_ref, "resourceMode": "fast", "userLimit": 4 }),
    )?;
    assert!(raised.error.is_none());
    assert_eq!(raised.body["outcome"], "accepted");
    assert_eq!(raised.body["lanes"][0]["effectiveLimit"], 4);

    let replaced = update_resource(
        harness,
        Channel::Gui,
        json!({ "jobRef": job_ref, "resourceMode": "quiet" }),
    )?;
    assert!(replaced.error.is_none());
    assert_eq!(replaced.body["outcome"], "accepted");
    assert_eq!(replaced.body["lanes"][0]["effectiveLimit"], 2);

    let duplicate: ScanStart = serde_json::from_value(
        start(
            harness,
            draft,
            fingerprint,
            Some(json!({ "resourceMode": "fast", "userLimit": 1 })),
        )?
        .body,
    )?;
    assert!(duplicate.existing);
    let current = duplicate.collection.ok_or("missing current condition")?;
    assert_eq!(
        current.resource_mode,
        du_core_contracts::analysis_ipc::ScanResourceMode::Quiet
    );
    assert_eq!(
        current.user_limit, None,
        "omission must clear the old limit"
    );
    Ok(())
}

fn assert_resource_target_and_channel_errors(
    harness: &ScanHarness,
    job_ref: &Id,
) -> Result<(), Box<dyn std::error::Error>> {
    let missing = update_resource(
        harness,
        Channel::Gui,
        json!({ "jobRef": Id::from_v7_bytes([0x77; 16]), "resourceMode": "auto" }),
    )?;
    assert_eq!(
        missing.error.ok_or("unknown job was accepted")?.reason_key,
        "job.not_found"
    );
    let cli = update_resource(
        harness,
        Channel::Cli,
        json!({ "jobRef": job_ref, "resourceMode": "auto" }),
    )?;
    assert_eq!(
        cli.error.ok_or("CLI control was accepted")?.reason_key,
        "command.not_provided"
    );
    Ok(())
}

fn finish_resource_scan(
    harness: &ScanHarness,
    job_ref: &Id,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(release) = &harness.priority_visit_release {
        release.send(())?;
    }
    harness.release.send(())?;
    let (summary, _) = scan_finished(&harness.terminal)?;
    assert_eq!(summary.job_id, *job_ref);
    let terminal = update_resource(
        harness,
        Channel::Gui,
        json!({ "jobRef": job_ref, "resourceMode": "auto" }),
    )?;
    assert!(terminal.error.is_none(), "{:?}", terminal.error);
    assert_eq!(terminal.body["outcome"], "already_terminal");
    Ok(())
}

#[test]
fn scan_set_resource_replaces_limits_reports_lanes_and_enforces_gui_ownership()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = fixture::scan_harness_with_priority()?;
    let draft = draft();
    let review = review(&harness, &draft)?;
    let started: ScanStart = serde_json::from_value(
        start(
            &harness,
            &draft,
            &review.review_fingerprint,
            Some(json!({ "resourceMode": "quiet", "userLimit": 2 })),
        )?
        .body,
    )?;
    harness
        .priority_opened
        .as_ref()
        .ok_or("missing scan open gate")?
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| {
            format!(
                "priority child open did not start ({error:?}); root opens {}, reads {}",
                harness.roots.opens.load(Ordering::Acquire),
                harness.roots.reads.load(Ordering::Acquire)
            )
        })?;
    let job_ref = started.job_ref.clone();
    assert_resource_updates(&harness, &draft, &review.review_fingerprint, &job_ref)?;
    assert_resource_target_and_channel_errors(&harness, &job_ref)?;
    finish_resource_scan(&harness, &started.job_ref)?;
    Ok(())
}

mod device_limits;
