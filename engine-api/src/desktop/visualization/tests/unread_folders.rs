//! The folders a treemap's scope could not read (#346, F06 README 48): counted apart
//! from the items not drawn, with the table row of the first one.
use super::lower_bound::{Entry, count, item_no, publish, render_in};
use super::support::Harness;
use super::{TestResult, rows_of};
use du_core_contracts::result::ResultCompleteness;
use serde_json::Value;

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
