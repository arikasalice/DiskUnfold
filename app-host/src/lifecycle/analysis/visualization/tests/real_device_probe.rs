//! #189: a real-device probe for the macOS checks that hosted runners cannot produce.
//!
//! Ignored by default. With `DU_PROBE_SCOPE` set, it scans that folder through the
//! production assembly and prints one `F03_MAC_PROBE` JSON line per fact: the scope review,
//! the finished job, the scope space total, the scan failures and the treemap answer on the
//! unique physical basis.
//! - `DU_PROBE_EXCLUDES`: colon-separated absolute paths excluded from the scope.
//! - `DU_PROBE_DURING`: a shell command run `DU_PROBE_DELAY_MS` (default 200) after the scan
//!   starts, for example `hdiutil detach -force /Volumes/probe` to remove a device mid-scan.
use std::time::{Duration, Instant};

use du_core_contracts::{EventEnvelope, Id, PathBytes};
use serde_json::{Value, json};

use super::{Assembled, Failure, assemble, ok};

fn native(path: &str) -> Value {
    json!({
        "encoding": "unix", "raw": PathBytes::from_bytes(path.as_bytes()),
        "display": path, "lossy": false,
    })
}

fn report(fact: &str, value: &Value) {
    eprintln!("F03_MAC_PROBE {}", json!({"fact": fact, "value": value}));
}

fn finished(assembled: &Assembled, job: &Id, limit: Duration) -> Result<EventEnvelope, Failure> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        match assembled.events.next_json() {
            Some(json) => {
                let event: EventEnvelope = serde_json::from_str(&json)?;
                if event.topic == "job.finished" && event.job_ref.as_ref() == Some(job) {
                    return Ok(event);
                }
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    Err("timed out waiting for the job to finish".into())
}

/// Runs the optional mid-scan command and reports when it ran and how it ended.
fn during(started: Instant) -> Result<(), Failure> {
    let Ok(command) = std::env::var("DU_PROBE_DURING") else {
        return Ok(());
    };
    let delay = std::env::var("DU_PROBE_DELAY_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);
    std::thread::sleep(Duration::from_millis(delay));
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&command)
        .status()?;
    report(
        "during",
        &json!({
            "command": command,
            "afterMs": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "status": status.code(),
        }),
    );
    Ok(())
}

/// The treemap answer of the scope on the unique physical basis, after any render job.
fn unique_physical_render(assembled: &Assembled, revision: &Id) -> Result<Value, Failure> {
    let view = ok(&assembled.engine, "viz.view.open", json!({}))?["viewChannel"].clone();
    let scope = json!({"resultVersion": revision, "itemNo": 0});
    let request = |seq: u32| {
        json!({
            "viewChannel": view, "requestSeq": seq, "format": "treemap", "resultRevision": revision,
            "aggregateScopeRef": scope, "navigationScopeRef": scope,
            "measure": "size", "sizeBasis": "unique_physical",
            "classificationCriterion": "none", "colorCriterion": "none",
            "denominatorBasis": "scope", "viewport": {"width": 800, "height": 600, "scale": 1.0},
            "visibleWindow": {"left": 0, "top": 0, "right": 1_000_000, "bottom": 1_000_000},
            "minAreaPx2": 16, "depthLimit": 1,
        })
    };
    let mut answer = ok(&assembled.engine, "viz.render", request(1))?;
    for seq in 2..8 {
        if answer["kind"] != "job" {
            break;
        }
        let job: Id = serde_json::from_value(answer["jobRef"].clone())?;
        finished(assembled, &job, Duration::from_secs(60))?;
        answer = ok(&assembled.engine, "viz.render", request(seq))?;
    }
    Ok(json!({"kind": answer["kind"], "reasonKey": answer["reasonKey"]}))
}

#[test]
#[ignore = "needs a real Mac and DU_PROBE_SCOPE"]
fn probe_a_real_macos_scan() -> Result<(), Failure> {
    let Ok(scope) = std::env::var("DU_PROBE_SCOPE") else {
        eprintln!("skipping: DU_PROBE_SCOPE is not set");
        return Ok(());
    };
    let exclusions: Vec<Value> = std::env::var("DU_PROBE_EXCLUDES")
        .map(|list| {
            list.split(':')
                .filter(|path| !path.is_empty())
                .map(native)
                .collect()
        })
        .unwrap_or_default();
    let assembled = assemble()?;
    let draft = json!({
        "includes": [native(&scope)], "exclusions": exclusions, "boundary": "current_volume",
        "includedMounts": [], "source": "gui",
    });
    let review = ok(
        &assembled.engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    report("scope.review", &review);
    let started = Instant::now();
    let start = ok(
        &assembled.engine,
        "scan.start",
        json!({"draft": draft, "reviewFingerprint": review["reviewFingerprint"]}),
    )?;
    let job: Id = serde_json::from_value(start["jobRef"].clone())?;
    during(started)?;
    let event = finished(&assembled, &job, Duration::from_secs(900))?;
    report(
        "job.finished",
        &json!({
            "body": event.body, "resultVersion": event.result_version,
            "elapsedMs": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        }),
    );
    let revision = event.result_version.ok_or("missing result revision")?;
    let total = ok(
        &assembled.engine,
        "space.total",
        json!({
            "resultVersion": revision, "requestSeq": 1,
            "target": {"kind": "folder", "item": {"resultVersion": revision, "itemNo": 0}},
        }),
    )?;
    report("space.total", &total);
    let failures = ok(
        &assembled.engine,
        "scan.failures",
        json!({"resultVersion": revision, "count": 1000}),
    )?;
    report("scan.failures", &failures);
    report(
        "viz.render.unique_physical",
        &unique_physical_render(&assembled, &revision)?,
    );
    Ok(())
}
