//! The folders a treemap's scope could not read (#346, F06 README 48): counted apart
//! from the items not drawn, with the table row of the first one.
use super::lower_bound::{Entry, count, item_no, publish, render_in};
use super::support::{Harness, render_body};
use super::{TestResult, rows_of};
use du_core_contracts::result::ResultCompleteness;
use serde_json::{Value, json};

fn publish_denied(harness: &Harness) -> Result<(), String> {
    publish(
        &harness.sink,
        &[
            Entry::Dir("a"),
            Entry::File("a/fa", 100),
            Entry::Dir("b"),
            Entry::File("b/fb", 50),
            Entry::Dir("b/locked"),
            Entry::Dir("denied"),
        ],
        ResultCompleteness::Missing,
        &["denied", "b/locked"],
    )
}

fn position(rows: &Value, item: u64) -> Option<u64> {
    rows["rows"]
        .as_array()?
        .iter()
        .position(|row| row["itemRef"]["itemNo"].as_u64() == Some(item))
        .map(|index| index as u64)
}

#[test]
fn unread_folders_are_counted_with_the_row_of_the_first_one() -> TestResult {
    let harness = Harness::product(2000)?;
    publish_denied(&harness)?;
    let view = harness.open_view()?;
    let answer = render_in(&harness, &view, 1, 0)?;
    let model = &answer["model"];
    let folders = &model["summary"]["unreadFolders"];
    assert_eq!(folders["unit"], "items", "{folders}");
    assert_eq!(folders["state"], "exact", "{folders}");
    assert_eq!(count(model, "unreadFolders")?, 2);
    // They are not items without an area: the two counts never overlap.
    assert_eq!(count(model, "unknownItems")?, 0);
    let rows = rows_of(&harness, &answer)?;
    // b holds an unread folder and comes before the empty unread folder "denied".
    let b = position(&rows, item_no(&rows, "b")?).ok_or("no row for b")?;
    let denied = position(&rows, item_no(&rows, "denied")?).ok_or("no row for denied")?;
    assert!(b < denied, "{rows}");
    assert_eq!(model["summary"]["firstUnreadRow"].as_u64(), Some(b));
    Ok(())
}

#[test]
fn a_scope_with_nothing_unread_has_neither_field() -> TestResult {
    let harness = Harness::product(2000)?;
    publish_denied(&harness)?;
    let view = harness.open_view()?;
    let root = rows_of(&harness, &render_in(&harness, &view, 1, 0)?)?;
    let a = item_no(&root, "a")?;
    let answer = render_in(&harness, &view, 2, a)?;
    let summary = &answer["model"]["summary"];
    assert!(summary.get("unreadFolders").is_none(), "{summary}");
    assert!(summary.get("firstUnreadRow").is_none(), "{summary}");
    Ok(())
}

fn row_named<'a>(rows: &'a Value, name: &str) -> Result<&'a Value, String> {
    rows["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["displayName"] == name))
        .ok_or_else(|| format!("no row named {name}"))
}

#[test]
fn folder_rows_say_whether_they_or_something_inside_could_not_be_read() -> TestResult {
    let harness = Harness::product(2000)?;
    publish_denied(&harness)?;
    let view = harness.open_view()?;
    let answer = render_in(&harness, &view, 1, 0)?;
    let rows = rows_of(&harness, &answer)?;
    assert_eq!(
        row_named(&rows, "denied")?["unread"],
        json!({"kind": "itself"})
    );
    assert_eq!(
        row_named(&rows, "b")?["unread"],
        json!({"kind": "inside", "count": "1"})
    );
    assert!(row_named(&rows, "a")?.get("unread").is_none());
    Ok(())
}

#[test]
fn an_expanded_folders_table_says_what_its_own_table_says() -> TestResult {
    let harness = Harness::product(2000)?;
    // c is drawn inside b and holds a folder that could not be read.
    publish(
        &harness.sink,
        &[
            Entry::Dir("a"),
            Entry::File("a/fa", 100),
            Entry::Dir("b"),
            Entry::Dir("b/c"),
            Entry::File("b/c/fc", 50),
            Entry::Dir("b/c/locked"),
        ],
        ResultCompleteness::Missing,
        &["b/c/locked"],
    )?;
    let view = harness.open_view()?;
    let mut body = render_body(&view, 1, "treemap", 3);
    body["depthLimit"] = json!(2);
    let answer = harness.ok("viz.render", body)?;
    let root = rows_of(&harness, &answer)?;
    let b = item_no(&root, "b")?;
    let shape = answer["model"]["shapes"]
        .as_array()
        .and_then(|shapes| {
            shapes
                .iter()
                .find(|shape| shape["itemRef"]["itemNo"].as_u64() == Some(b))
        })
        .ok_or("no shape for b")?;
    let expanded = harness.ok(
        "viz.rows",
        json!({
            "modelRef": answer["model"]["modelRef"], "resultRevision": answer["resultRevision"],
            "start": 0, "count": 1000, "parentShapeRef": shape["shapeRef"],
        }),
    )?;
    let own = rows_of(&harness, &render_in(&harness, &view, 2, b)?)?;
    let inside = row_named(&expanded, "c")?;
    assert_eq!(inside["unread"], json!({"kind": "inside", "count": "1"}));
    assert_eq!(inside["unread"], row_named(&own, "c")?["unread"]);
    Ok(())
}
