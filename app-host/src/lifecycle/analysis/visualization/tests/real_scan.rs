//! A real scan of a temporary folder, drawn and listed through the assembled engine.
use std::{
    fs,
    time::{Duration, Instant},
};

use du_core_contracts::{EventEnvelope, Id, PathBytes};
use du_engine_api::desktop::DesktopEvents;
use serde_json::{Value, json};

use super::{Assembled, Failure, assemble, ok};

mod size_rule;

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

/// A flat folder of files with different sizes; the ranked bar needs file children only.
fn flat_folder() -> Result<tempfile::TempDir, Failure> {
    let folder = tempfile::tempdir()?;
    fs::write(folder.path().join("a.bin"), vec![0_u8; 17])?;
    fs::write(folder.path().join("b.bin"), vec![0_u8; 31])?;
    fs::write(folder.path().join("c.bin"), vec![0_u8; 5])?;
    Ok(folder)
}

/// Renders `format` on a view channel of its own, because request numbers count per channel.
fn render_model(assembled: &Assembled, revision: &Id, format: &str) -> Result<Value, Failure> {
    let view = ok(&assembled.engine, "viz.view.open", json!({}))?["viewChannel"].clone();
    let answer = render_settled(assembled, &view, revision, format)?;
    assert_eq!(answer["kind"], "model", "{format}: {answer}");
    Ok(answer["model"].clone())
}

fn rows_page(
    assembled: &Assembled,
    model: &Value,
    revision: &Id,
    start: u32,
    count: u32,
) -> Result<Value, Failure> {
    ok(
        &assembled.engine,
        "viz.rows",
        json!({
            "modelRef": model["modelRef"], "resultRevision": revision,
            "start": start, "count": count,
        }),
    )
}

fn byte_count(value: &Value) -> Result<u64, Failure> {
    Ok(value.as_str().ok_or("not a decimal string")?.parse()?)
}

fn locate(assembled: &Assembled, model: &Value, item: &Value) -> Result<Value, Failure> {
    ok(
        &assembled.engine,
        "viz.locate",
        json!({"modelRef": model["modelRef"], "itemRef": item}),
    )
}

/// An item number that no scan of the test folder produces.
fn unknown_item(revision: &Id) -> Value {
    json!({"resultVersion": revision, "itemNo": 99})
}

#[test]
fn treemap_and_bar_rows_and_locations_come_from_the_rendered_model() -> Result<(), Failure> {
    let assembled = assemble()?;
    let folder = flat_folder()?;
    let revision = scan(&assembled, folder.path())?;
    for format in ["treemap", "bar"] {
        let model = render_model(&assembled, &revision, format)?;
        let page = rows_page(&assembled, &model, &revision, 0, 100)?;
        let rows = page["rows"].as_array().ok_or("rows are not a list")?;
        assert_eq!(page["totalCount"], 3, "{format}");
        // The sibling sizes differ, so the order is the size order.
        let sizes = rows
            .iter()
            .map(|row| byte_count(&row["size"]["bytes"]))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(sizes, [31, 17, 5], "{format}");
        assert_eq!(
            byte_count(&model["totals"]["scope"]["quantity"]["value"])?,
            53,
            "{format}"
        );
        // A later page is the same row the first page listed at that position.
        let later = rows_page(&assembled, &model, &revision, 1, 1)?;
        assert_eq!(later["rows"], json!([rows.get(1)]), "{format}");
        // Every listed item is found on the drawn shape that carries its reference.
        let shapes = model["shapes"].as_array().ok_or("shapes are not a list")?;
        for row in rows {
            let located = locate(&assembled, &model, &row["itemRef"])?;
            assert_eq!(located["location"], "shape", "{format}");
            let index = usize::try_from(located["shapeIndex"].as_u64().ok_or("no shape index")?)?;
            let shape = shapes.get(index).ok_or("shape index out of range")?;
            assert_eq!(shape["itemRef"], row["itemRef"], "{format}");
        }
        let missing = locate(&assembled, &model, &unknown_item(&revision));
        assert_eq!(
            missing.err().map(|error| error.to_string()).as_deref(),
            Some("viz.locate: ref.unavailable"),
            "{format}"
        );
    }
    Ok(())
}

#[test]
fn histogram_interval_rows_and_their_members_add_up_to_the_scope_total() -> Result<(), Failure> {
    let assembled = assemble()?;
    let folder = flat_folder()?;
    let revision = scan(&assembled, folder.path())?;
    let model = render_model(&assembled, &revision, "histogram")?;
    let page = rows_page(&assembled, &model, &revision, 0, 100)?;
    let rows = page["rows"].as_array().ok_or("rows are not a list")?;
    assert!(!rows.is_empty());
    assert_eq!(page["totalCount"], rows.len());
    // A row stands for an interval: it has a group reference and no item reference.
    assert!(
        rows.iter()
            .all(|row| !row["groupRef"].is_null() && row["itemRef"].is_null())
    );
    let mut total = 0;
    for row in rows {
        total += byte_count(&row["quantity"]["value"])?;
    }
    assert_eq!(total, 53);
    assert_eq!(
        byte_count(&model["totals"]["scope"]["quantity"]["value"])?,
        53
    );
    // The three files are the members of the one interval that holds bytes.
    let holding = rows
        .iter()
        .find(|row| row["quantity"]["value"] == "53")
        .ok_or("no interval holds the files")?;
    let members = ok(
        &assembled.engine,
        "viz.group_rows",
        json!({
            "modelRef": model["modelRef"], "groupRef": holding["groupRef"],
            "start": 0, "count": 10,
        }),
    )?;
    let listed = members["rows"].as_array().ok_or("members are not a list")?;
    let mut sizes = listed
        .iter()
        .map(|row| byte_count(&row["size"]["bytes"]))
        .collect::<Result<Vec<_>, _>>()?;
    sizes.sort_unstable();
    assert_eq!(sizes, [5, 17, 31]);
    assert!(listed.iter().all(|row| !row["itemRef"].is_null()));
    let missing = locate(&assembled, &model, &unknown_item(&revision));
    assert_eq!(
        missing.err().map(|error| error.to_string()).as_deref(),
        Some("viz.locate: ref.unavailable")
    );
    Ok(())
}
