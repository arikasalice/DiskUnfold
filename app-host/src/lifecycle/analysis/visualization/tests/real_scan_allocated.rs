//! A real scan of a temporary folder, drawn on the allocated and the logical size basis.
//!
//! The helpers repeat the ones of `real_scan` so that the two files can change apart.
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

use du_core_contracts::{EventEnvelope, Id, PathBytes, visualization::model::VizModel};
use du_engine_api::desktop::DesktopEvents;
use serde_json::{Value, json};

use super::{Assembled, Failure, assemble, ok};

/// A scope path in the encoding this platform's scan adapters read.
#[cfg(windows)]
fn native(path: &Path) -> Value {
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
fn native(path: &Path) -> Value {
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
fn scan(assembled: &Assembled, folder: &Path) -> Result<Id, Failure> {
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

/// Renders the treemap of the scanned folder on `basis`, on a view channel of its own.
fn render(assembled: &Assembled, revision: &Id, basis: &str) -> Result<Value, Failure> {
    let view = ok(&assembled.engine, "viz.view.open", json!({}))?["viewChannel"].clone();
    let scope = json!({"resultVersion": revision, "itemNo": 0});
    let request = |seq: u32| {
        json!({
            "viewChannel": view, "requestSeq": seq, "format": "treemap", "resultRevision": revision,
            "aggregateScopeRef": scope, "navigationScopeRef": scope,
            "measure": "size", "sizeBasis": basis,
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

/// The rows of the model, or of the table-only fallback, that an answer refers to.
fn rows_of(assembled: &Assembled, answer: &Value) -> Result<Value, Failure> {
    let (model_ref, revision) = if answer["kind"] == "model" {
        (&answer["model"]["modelRef"], &answer["resultRevision"])
    } else {
        (
            &answer["fallback"]["modelRef"],
            &answer["fallback"]["resultRevision"],
        )
    };
    ok(
        &assembled.engine,
        "viz.rows",
        json!({"modelRef": model_ref, "resultRevision": revision, "start": 0, "count": 100}),
    )
}

fn bytes(value: &Value) -> Result<u64, Failure> {
    Ok(value.as_str().ok_or("not a decimal string")?.parse()?)
}

/// The model survives the wire and passes the contract checks, which include the sum of
/// its totals.
fn assert_valid(model: &Value, revision: &Id) -> Result<(), Failure> {
    let parsed: VizModel = serde_json::from_value(model.clone())?;
    parsed.validate_revision(revision).map_err(Failure::from)
}

/// The exact remainder of the model's totals.
fn remainder(model: &Value) -> Result<u64, Failure> {
    let quantity = &model["totals"]["remainder"]["quantity"];
    assert_eq!(quantity["state"], "exact", "{}", model["totals"]);
    bytes(&quantity["value"])
}

/// The F03 total of the scanned folder on `basis`.
fn space_total(
    assembled: &Assembled,
    revision: &Id,
    basis: &str,
    seq: u32,
) -> Result<u64, Failure> {
    let space = ok(
        &assembled.engine,
        "space.total",
        json!({
            "resultVersion": revision, "requestSeq": seq,
            "target": {"kind": "folder", "item": {"resultVersion": revision, "itemNo": 0}},
        }),
    )?;
    let entry = space["sizes"]
        .as_array()
        .and_then(|sizes| sizes.iter().find(|entry| entry["size"]["basis"] == basis))
        .ok_or("no total on this basis")?;
    bytes(&entry["size"]["bytes"])
}

/// Three files of different sizes and a folder, so that no ordering tie is involved.
fn four_children() -> Result<tempfile::TempDir, Failure> {
    let folder = tempfile::tempdir()?;
    fs::create_dir(folder.path().join("sub"))?;
    fs::write(folder.path().join("a.bin"), vec![0_u8; 51_200])?;
    fs::write(folder.path().join("b.bin"), vec![0_u8; 30_000])?;
    fs::write(folder.path().join("c.bin"), vec![0_u8; 9_001])?;
    fs::write(folder.path().join("sub").join("d.bin"), vec![0_u8; 20_000])?;
    Ok(folder)
}

// A scan records no extent-sharing evidence for any file, and the treemap used to refuse
// every file for that, so the allocated basis (the view's default) never drew a shape.
#[test]
fn a_real_scan_draws_the_treemap_on_the_allocated_and_the_logical_basis() -> Result<(), Failure> {
    let assembled = assemble()?;
    let folder = four_children()?;
    let revision = scan(&assembled, folder.path())?;
    for (seq, basis) in (1..).zip(["allocated", "logical"]) {
        let answer = render(&assembled, &revision, basis)?;
        assert_eq!(answer["kind"], "model", "{basis}: {answer}");
        let model = &answer["model"];
        // One shape for each of the four children, and none counted as unknown.
        assert_eq!(model["shapes"].as_array().map(Vec::len), Some(4), "{basis}");
        assert_eq!(model["summary"]["unknownItems"]["value"], "0", "{basis}");
        // The scope total is the F03 value, and the children and the drawn shapes add up to it.
        let scope = bytes(&model["totals"]["scope"]["quantity"]["value"])?;
        assert_eq!(
            scope,
            space_total(&assembled, &revision, basis, seq)?,
            "{basis}"
        );
        assert_eq!(
            bytes(&model["totals"]["drawn"]["quantity"]["value"])?,
            scope,
            "{basis}"
        );
        // Nothing is left out: the remainder is exactly zero and names no cause.
        assert_eq!(remainder(model)?, 0, "{basis}");
        assert!(
            model["totals"].get("remainderReasonKeys").is_none(),
            "{basis}"
        );
        assert_valid(model, &revision)?;
        let rows = rows_of(&assembled, &answer)?;
        assert_eq!(rows["totalCount"], 4, "{basis}");
        let listed = rows["rows"]
            .as_array()
            .ok_or("rows are not a list")?
            .iter()
            .map(|row| bytes(&row["size"]["bytes"]))
            .sum::<Result<u64, _>>()?;
        assert_eq!(listed, scope, "{basis}");
    }
    Ok(())
}

// Two links of one file in sibling folders cannot both be given the file's bytes. The
// allocated basis decides child by child: the two folders that share the file are unknown
// items without a shape, the folder that shares nothing is drawn, and the parent stays a
// model. The bytes of the shared file stay in the scope total and in no drawn shape. The
// logical basis counts bytes by path and draws all three folders.
#[test]
fn a_file_linked_from_two_sibling_folders_is_unknown_for_both_on_the_allocated_basis()
-> Result<(), Failure> {
    let assembled = assemble()?;
    let folder = tempfile::tempdir()?;
    for (name, file, size) in [("left", "one.bin", 40_960), ("other", "three.bin", 20_000)] {
        fs::create_dir(folder.path().join(name))?;
        fs::write(folder.path().join(name).join(file), vec![0_u8; size])?;
    }
    fs::create_dir(folder.path().join("right"))?;
    fs::hard_link(
        folder.path().join("left").join("one.bin"),
        folder.path().join("right").join("two.bin"),
    )?;
    let revision = scan(&assembled, folder.path())?;

    let allocated = render(&assembled, &revision, "allocated")?;
    assert_eq!(allocated["kind"], "model", "{allocated}");
    let model = &allocated["model"];
    let rows = rows_of(&assembled, &allocated)?;
    assert_eq!(rows["totalCount"], 3);
    let row_bytes = |name: &str| -> Result<u64, Failure> {
        let row = rows["rows"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["displayName"] == name))
            .ok_or("no row of this name")?;
        bytes(&row["size"]["bytes"])
    };
    // Only the folder that shares nothing is drawn; the other two are counted, not drawn.
    let shapes = model["shapes"].as_array().ok_or("shapes are not a list")?;
    assert_eq!(shapes.len(), 1, "{allocated}");
    let other = rows["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["displayName"] == "other"))
        .ok_or("no row of the folder that shares nothing")?;
    assert_eq!(shapes[0]["itemRef"], other["itemRef"]);
    assert_eq!(model["summary"]["unknownItems"]["value"], "2");
    // The scope total is the F03 value, which counts the shared file once. The drawn total
    // is the one folder, and what is left of the scope is that file.
    let scope = bytes(&model["totals"]["scope"]["quantity"]["value"])?;
    assert_eq!(scope, space_total(&assembled, &revision, "allocated", 1)?);
    let drawn = bytes(&model["totals"]["drawn"]["quantity"]["value"])?;
    assert_eq!(drawn, row_bytes("other")?);
    assert_eq!(bytes(&model["totals"]["grouped"]["quantity"]["value"])?, 0);
    assert_eq!(scope - drawn, row_bytes("left")?);
    assert_eq!(row_bytes("left")?, row_bytes("right")?);
    // What is left of the scope is the shared file, counted once, and no folder carries
    // it: the remainder says so, and the summary counts the two folders it could not draw.
    assert_eq!(remainder(model)?, row_bytes("left")?);
    assert_eq!(
        model["totals"]["remainderReasonKeys"],
        json!(["viz.remainder.unattributed"])
    );
    assert_valid(model, &revision)?;

    let logical = render(&assembled, &revision, "logical")?;
    assert_eq!(logical["kind"], "model", "{logical}");
    assert_eq!(logical["model"]["shapes"].as_array().map(Vec::len), Some(3));
    assert_eq!(logical["model"]["summary"]["unknownItems"]["value"], "0");
    // Counted by path, all three folders are drawn and nothing is left over.
    assert_eq!(remainder(&logical["model"])?, 0);
    assert!(
        logical["model"]["totals"]
            .get("remainderReasonKeys")
            .is_none()
    );
    assert_valid(&logical["model"], &revision)?;
    Ok(())
}
