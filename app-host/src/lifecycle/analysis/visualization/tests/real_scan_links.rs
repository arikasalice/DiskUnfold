//! Real scans of temporary folders that hold a link or an unreadable folder, drawn on the
//! logical and on the allocated basis through the assembled engine.
//!
//! A link that was not followed, and a range that could not be read, each leave a value
//! without bytes, so the F03 value above it is not a plain exact value. The treemap
//! draws such a scope instead of falling back to the table. A scan records no
//! extent-sharing evidence, so the allocated basis is attributed by file identity and
//! link count alone; each scan below is checked on both bases.
//!
//! Windows only: the links are junctions made with mklink, and the unreadable folder is
//! made with an ACL. A fixture that cannot be made is reported on stderr and skipped.
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use du_core_contracts::{EventEnvelope, Id, PathBytes};
use du_engine_api::desktop::DesktopEvents;
use serde_json::{Value, json};

use super::{Assembled, Failure, assemble, ok};

const LOWER_BOUND_FLAG: &str = "viz.flag.lower_bound";

/// The size bases a treemap is drawn on. Each scan is rendered once on each.
const BASES: [&str; 2] = ["logical", "allocated"];

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

fn finished(events: &DesktopEvents, job: &Id) -> Result<EventEnvelope, Failure> {
    let deadline = Instant::now() + Duration::from_secs(30);
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
    Err("timed out waiting for the job to finish".into())
}

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

/// Renders the treemap of one folder of the scan on `basis`.
fn render(assembled: &Assembled, revision: &Id, item: u64, basis: &str) -> Result<Value, Failure> {
    let view = ok(&assembled.engine, "viz.view.open", json!({}))?["viewChannel"].clone();
    let scope = json!({"resultVersion": revision, "itemNo": item});
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

fn rows_of(assembled: &Assembled, answer: &Value) -> Result<Value, Failure> {
    ok(
        &assembled.engine,
        "viz.rows",
        json!({
            "modelRef": answer["model"]["modelRef"], "resultRevision": answer["resultRevision"],
            "start": 0, "count": 100,
        }),
    )
}

fn row<'a>(rows: &'a Value, name: &str) -> Result<&'a Value, Failure> {
    rows["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["displayName"] == name))
        .ok_or_else(|| format!("no row named {name}").into())
}

fn shape_of<'a>(model: &'a Value, item: &Value) -> Result<&'a Value, Failure> {
    model["shapes"]
        .as_array()
        .and_then(|shapes| {
            shapes
                .iter()
                .find(|shape| shape["itemRef"]["itemNo"] == *item)
        })
        .ok_or_else(|| format!("no shape for item {item}").into())
}

fn number(value: &Value) -> Result<u64, Failure> {
    Ok(value.as_str().ok_or("not a decimal string")?.parse()?)
}

fn count(model: &Value, name: &str) -> Result<u64, Failure> {
    number(&model["summary"][name]["value"])
}

/// The F03 total of one folder of the scan on `basis`.
fn space_total(
    assembled: &Assembled,
    revision: &Id,
    item: u64,
    basis: &str,
) -> Result<u64, Failure> {
    let space = ok(
        &assembled.engine,
        "space.total",
        json!({
            "resultVersion": revision, "requestSeq": 1,
            "target": {"kind": "folder", "item": {"resultVersion": revision, "itemNo": item}},
        }),
    )?;
    let entry = space["sizes"]
        .as_array()
        .and_then(|sizes| sizes.iter().find(|entry| entry["size"]["basis"] == basis))
        .ok_or("no total on this basis")?;
    number(&entry["size"]["bytes"])
}

fn write(path: &Path, size: usize) -> Result<(), Failure> {
    fs::write(path, vec![0_u8; size])?;
    Ok(())
}

fn junction(link: &Path, target: &Path) -> Result<bool, Failure> {
    let out = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()?;
    if !out.status.success() {
        eprintln!(
            "skipped: mklink /J failed: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
    Ok(out.status.success())
}

// F03 lowers the value above a link that was not followed to a lower bound today, and
// FR5.HD1 is to be revised so that it stays exact and the link is an exact zero. This test
// holds for both: it asserts what the treemap does with a link and never the value state
// of the folder above it, except that the mark on a shape agrees with the state of its
// own row. It holds on the logical and on the allocated basis.
#[test]
fn a_real_scan_with_junctions_draws_the_siblings_and_gives_each_link_no_area() -> Result<(), Failure>
{
    let root = tempfile::tempdir()?;
    let elsewhere = tempfile::tempdir()?;
    fs::create_dir(elsewhere.path().join("target"))?;
    write(&elsewhere.path().join("target").join("t.bin"), 70_000)?;
    fs::create_dir(root.path().join("clean"))?;
    write(&root.path().join("clean").join("c1.bin"), 30_000)?;
    write(&root.path().join("plain.bin"), 20_000)?;
    fs::create_dir(root.path().join("holder"))?;
    write(&root.path().join("holder").join("h.bin"), 10_000)?;
    let target = elsewhere.path().join("target");
    if !junction(&root.path().join("holder").join("jlink"), &target)?
        || !junction(&root.path().join("toplink"), &target)?
    {
        return Ok(());
    }
    let assembled = assemble()?;
    let revision = scan(&assembled, root.path())?;
    for basis in BASES {
        check_junction_scope(&assembled, &revision, basis)?;
    }
    Ok(())
}

fn check_junction_scope(assembled: &Assembled, revision: &Id, basis: &str) -> Result<(), Failure> {
    let answer = render(assembled, revision, 0, basis)?;
    assert_eq!(answer["kind"], "model", "{basis}: {answer}");
    let model = &answer["model"];
    // The three siblings that have bytes are drawn; the link beside them has no area. It
    // is an item without a value, which the summary counts, or an exact zero (FR5.HD1
    // revised), which has no area either.
    let shapes = model["shapes"].as_array().map(Vec::len);
    assert_eq!(shapes, Some(3), "{basis}: {model}");
    let rows = rows_of(assembled, &answer)?;
    assert_eq!(rows["totalCount"], 4, "{basis}");
    let link = row(&rows, "toplink")?;
    let without_value = link["size"]["state"] == "unknown";
    assert!(
        without_value || (link["size"]["state"] == "exact" && link["size"]["bytes"] == "0"),
        "{basis}: {link}"
    );
    assert_eq!(
        count(model, "unknownItems")?,
        u64::from(without_value),
        "{basis}"
    );
    assert!(
        shape_of(model, &link["itemRef"]["itemNo"]).is_err(),
        "{basis}"
    );
    for name in ["clean", "plain.bin", "holder"] {
        assert_ne!(
            row(&rows, name)?["size"]["state"],
            "unknown",
            "{basis}: {name}"
        );
    }
    // The scope is the F03 value, and the link adds no bytes to what is drawn.
    let scope = number(&model["totals"]["scope"]["quantity"]["value"])?;
    assert_eq!(
        scope,
        space_total(assembled, revision, 0, basis)?,
        "{basis}"
    );
    if basis == "logical" {
        assert_eq!(scope, 60_000);
    }
    let drawn = number(&model["totals"]["drawn"]["quantity"]["value"])?;
    assert_eq!(drawn, scope, "{basis}");
    // A folder without a link is exact and unmarked; the mark of the folder above a link
    // follows the state of its row.
    let clean = row(&rows, "clean")?;
    assert_eq!(clean["size"]["state"], "exact", "{basis}");
    assert_eq!(
        shape_of(model, &clean["itemRef"]["itemNo"])?["displayFlags"],
        json!([]),
        "{basis}"
    );
    let holder = row(&rows, "holder")?;
    let marked = holder["size"]["state"] == "lower_bound";
    assert_eq!(
        shape_of(model, &holder["itemRef"]["itemNo"])?["displayFlags"],
        if marked {
            json!([LOWER_BOUND_FLAG])
        } else {
            json!([])
        },
        "{basis}"
    );
    let holder_item = holder["itemRef"]["itemNo"].as_u64().ok_or("no item")?;
    check_inside_the_holder(assembled, revision, holder_item, basis)
}

/// Inside the folder that holds a link, the file is drawn and the link has no shape.
fn check_inside_the_holder(
    assembled: &Assembled,
    revision: &Id,
    holder: u64,
    basis: &str,
) -> Result<(), Failure> {
    let inner = render(assembled, revision, holder, basis)?;
    assert_eq!(inner["kind"], "model", "{basis}: {inner}");
    let shapes = inner["model"]["shapes"].as_array().map(Vec::len);
    assert_eq!(shapes, Some(1), "{basis}");
    let inner_rows = rows_of(assembled, &inner)?;
    let without_value = inner_rows["rows"].as_array().map_or(0, |rows| {
        rows.iter()
            .filter(|row| row["size"]["state"] == "unknown")
            .count()
    });
    assert_eq!(
        count(&inner["model"], "unknownItems")?,
        without_value as u64,
        "{basis}"
    );
    Ok(())
}

/// Denies reading a folder for the current user and gives the access back when dropped,
/// so that the temporary tree can always be removed.
struct Denied<'a> {
    folder: &'a Path,
    user: String,
}

impl<'a> Denied<'a> {
    fn new(folder: &'a Path) -> Option<Self> {
        let user = std::env::var("USERNAME").ok()?;
        let status = Command::new("icacls")
            .arg(folder)
            .args(["/deny", &format!("{user}:(OI)(CI)(RX)")])
            .output()
            .ok()?
            .status;
        status.success().then_some(Self { folder, user })
    }
}

impl Drop for Denied<'_> {
    fn drop(&mut self) {
        let _ = Command::new("icacls")
            .arg(self.folder)
            .args(["/remove:d", &self.user])
            .output();
    }
}

// An unreadable folder makes the result a missing one. F03 then gives every value the
// lower bound of the whole result today, and only the folders above the unread range once
// FR5.HD1 is applied per folder. The treemap draws the scope instead of falling back to the
// table, names the range's own reason, and carries each value's state; this test holds for
// both, so it asserts that the marks and the totals follow the rows, never which rows are
// lower bounds. It holds on the logical and on the allocated basis.
#[test]
fn a_real_scan_with_an_unreadable_folder_draws_the_scope_and_names_the_reason()
-> Result<(), Failure> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("clean"))?;
    write(&root.path().join("clean").join("c1.bin"), 30_000)?;
    write(&root.path().join("plain.bin"), 20_000)?;
    let locked = root.path().join("locked");
    fs::create_dir(&locked)?;
    write(&locked.join("secret.bin"), 15_000)?;
    let Some(denied) = Denied::new(&locked) else {
        eprintln!("skipped: the access control list of the fixture could not be changed");
        return Ok(());
    };
    let assembled = assemble()?;
    let scanned = scan(&assembled, root.path());
    drop(denied);
    let revision = scanned?;
    let issues = ok(
        &assembled.engine,
        "scan.failures",
        json!({"resultVersion": revision, "count": 50}),
    )?;
    let denied_rows = issues["rows"].as_array().map_or(0, |rows| {
        rows.iter()
            .filter(|row| row["reasonKey"] == "scan.path_denied")
            .count()
    });
    if denied_rows == 0 {
        eprintln!("skipped: the scan was able to read the folder that was denied");
        return Ok(());
    }
    for basis in BASES {
        check_unreadable_scope(&assembled, &revision, basis)?;
    }
    Ok(())
}

fn check_unreadable_scope(
    assembled: &Assembled,
    revision: &Id,
    basis: &str,
) -> Result<(), Failure> {
    let answer = render(assembled, revision, 0, basis)?;
    assert_eq!(answer["kind"], "model", "{basis}: {answer}");
    let model = &answer["model"];
    // The two readable siblings are drawn; the unreadable one is at least zero bytes
    // (#341), so it has no area and is not counted as an unknown item.
    let shapes = model["shapes"].as_array().map(Vec::len);
    assert_eq!(shapes, Some(2), "{basis}: {model}");
    assert_eq!(count(model, "unknownItems")?, 0, "{basis}");
    assert_eq!(
        model["summary"]["reasonKeys"],
        json!(["scan.path_denied"]),
        "{basis}"
    );
    let rows = rows_of(assembled, &answer)?;
    assert_eq!(
        row(&rows, "locked")?["size"]["state"],
        "lower_bound",
        "{basis}"
    );
    // The unreadable folder is directly under the scope, so the scope is a lower bound.
    assert_eq!(
        model["totals"]["scope"]["quantity"]["state"], "lower_bound",
        "{basis}"
    );
    let scope = number(&model["totals"]["scope"]["quantity"]["value"])?;
    assert_eq!(
        scope,
        space_total(assembled, revision, 0, basis)?,
        "{basis}"
    );
    if basis == "logical" {
        assert_eq!(scope, 50_000);
    }
    let drawn = number(&model["totals"]["drawn"]["quantity"]["value"])?;
    assert_eq!(drawn, scope, "{basis}");
    check_marks_follow_rows(model, &rows, basis)
}

/// The mark of each shape and the state of the drawn total follow the rows they come from.
fn check_marks_follow_rows(model: &Value, rows: &Value, basis: &str) -> Result<(), Failure> {
    let mut any_lower = false;
    for shape in model["shapes"].as_array().ok_or("no shapes")? {
        let own = rows["rows"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["itemRef"] == shape["itemRef"]))
            .ok_or("no row for a shape")?;
        let lower = own["size"]["state"] == "lower_bound";
        any_lower |= lower;
        assert_eq!(
            shape["displayFlags"],
            if lower {
                json!([LOWER_BOUND_FLAG])
            } else {
                json!([])
            },
            "{basis}"
        );
    }
    assert_eq!(
        model["totals"]["drawn"]["quantity"]["state"] == "lower_bound",
        any_lower,
        "{basis}"
    );
    Ok(())
}
