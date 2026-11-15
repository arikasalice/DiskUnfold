//! A real scan of a temporary folder, drawn and listed through the assembled engine.
use std::{
    fs,
    time::{Duration, Instant},
};

use du_core_contracts::{EventEnvelope, Id, PathBytes};
use du_engine_api::desktop::DesktopEvents;
use serde_json::{Value, json};

use super::{Assembled, Failure, assemble, ok};

/// A scope path in the encoding this platform's scan adapters read.
#[cfg(windows)]
fn native(path: &std::path::Path) -> Value {
    use std::os::windows::ffi::OsStrExt;
    let bytes: Vec<u8> = path
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    json!({
        "encoding": "windows_utf16le", "raw": PathBytes::from_bytes(&bytes),
        "display": path.to_string_lossy(), "lossy": false,
    })
}

#[cfg(target_os = "linux")]
fn native(path: &std::path::Path) -> Value {
    use std::os::unix::ffi::OsStrExt;
    json!({
        "encoding": "unix", "raw": PathBytes::from_bytes(path.as_os_str().as_bytes()),
        "display": path.to_string_lossy(), "lossy": false,
    })
}

fn finished(events: &DesktopEvents, job: &Id) -> Result<EventEnvelope, Failure> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        match events.next_json() {
            Some(json) => {
                let event: EventEnvelope = serde_json::from_str(&json)?;
                if event.topic == "job.finished" && event.job_ref.as_ref() == Some(job) {
                    return Ok(event);
                }
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    Err("timed out waiting for the scan to finish".into())
}

/// Scans `folder` through the assembled engine and returns the result revision.
fn scan(assembled: &Assembled, folder: &std::path::Path) -> Result<Id, Failure> {
    let draft = json!({
        "includes": [native(folder)], "exclusions": [], "boundary": "current_volume",
        "includedMounts": [], "source": "gui",
    });
    let review = ok(
        &assembled.engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    let start = ok(
        &assembled.engine,
        "scan.start",
        json!({"draft": draft, "reviewFingerprint": review["reviewFingerprint"]}),
    )?;
    let job: Id = serde_json::from_value(start["jobRef"].clone())?;
    let event = finished(&assembled.events, &job)?;
    assert_eq!(event.body["state"], "done");
    Ok(event.result_version.ok_or("missing result revision")?)
}

/// Renders until the answer is no longer a job, re-asking after the job has finished.
fn render_settled(
    assembled: &Assembled,
    view: &Value,
    revision: &Id,
    format: &str,
) -> Result<Value, Failure> {
    let scope = json!({"resultVersion": revision, "itemNo": 0});
    let request = |seq: u32| {
        json!({
            "viewChannel": view, "requestSeq": seq, "format": format, "resultRevision": revision,
            "aggregateScopeRef": scope, "navigationScopeRef": scope,
            "measure": "size", "sizeBasis": "logical",
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
        finished(&assembled.events, &job)?;
        answer = ok(&assembled.engine, "viz.render", request(seq))?;
    }
    Ok(answer)
}

#[test]
fn a_real_scan_feeds_the_treemap_and_its_rows_the_same_totals() -> Result<(), Failure> {
    let assembled = assemble()?;
    let fixture = tempfile::tempdir()?;
    fs::create_dir(fixture.path().join("sub"))?;
    // Sibling sizes differ so that no ordering tie is involved.
    fs::write(fixture.path().join("a.bin"), vec![0_u8; 17])?;
    fs::write(fixture.path().join("b.bin"), vec![0_u8; 31])?;
    fs::write(fixture.path().join("sub").join("c.bin"), vec![0_u8; 5])?;
    let revision = scan(&assembled, fixture.path())?;
    let view = ok(&assembled.engine, "viz.view.open", json!({}))?["viewChannel"].clone();
    let answer = render_settled(&assembled, &view, &revision, "treemap")?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let rows = ok(
        &assembled.engine,
        "viz.rows",
        json!({
            "modelRef": answer["model"]["modelRef"], "resultRevision": revision,
            "start": 0, "count": 100,
        }),
    )?;
    let listed: u64 = rows["rows"]
        .as_array()
        .ok_or("rows are not a list")?
        .iter()
        .map(|row| {
            row["size"]["bytes"]
                .as_str()
                .and_then(|bytes| bytes.parse::<u64>().ok())
        })
        .sum::<Option<u64>>()
        .ok_or("a row has no byte value")?;
    assert_eq!(rows["totalCount"], 3);
    assert_eq!(listed, 53);
    assert_eq!(
        answer["model"]["totals"]["scope"]["quantity"]["value"],
        "53"
    );
    let space = ok(
        &assembled.engine,
        "space.total",
        json!({
            "resultVersion": revision, "requestSeq": 1,
            "target": {"kind": "folder", "item": {"resultVersion": revision, "itemNo": 0}},
        }),
    )?;
    let logical = space["sizes"]
        .as_array()
        .and_then(|sizes| {
            sizes
                .iter()
                .find(|entry| entry["size"]["basis"] == "logical")
        })
        .ok_or("no logical total")?;
    assert_eq!(logical["size"]["bytes"], "53");
    Ok(())
}
