//! Type slices on folder rows and shapes (#298, F06 README 54): the files of each
//! type below a folder, from one function for the rows and the shapes.
use super::lower_bound::{Entry, item_no, publish};
use super::support::{Harness, render_body};
use super::{TestResult, rows_of};
use crate::viz_production::policy;
use du_core_contracts::result::ResultCompleteness;
use serde_json::{Value, json};

fn by_type(harness: &Harness, view: &Value, seq: u32, format: &str) -> Result<Value, String> {
    let mut body = render_body(view, seq, format, 3);
    body["colorCriterion"] = json!(policy::COLOR_CRITERION_TYPE);
    let answer = harness.ok("viz.render", body)?;
    assert_eq!(answer["kind"], "model", "{answer}");
    Ok(answer)
}

fn row<'a>(rows: &'a Value, name: &str) -> Result<&'a Value, String> {
    rows["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["displayName"] == name))
        .ok_or_else(|| format!("no row named {name}"))
}

fn shape(answer: &Value, item: u64) -> Result<&Value, String> {
    answer["model"]["shapes"]
        .as_array()
        .and_then(|shapes| {
            shapes
                .iter()
                .find(|shape| shape["itemRef"]["itemNo"].as_u64() == Some(item))
        })
        .ok_or_else(|| format!("no shape for item {item}"))
}

fn slice(key: &str, state: &str, count: u64, bytes: u64) -> Value {
    json!({
        "classKey": key,
        "itemCount": {"unit": "items", "state": state, "value": count.to_string()},
        "total": {"unit": "bytes", "state": state, "value": bytes.to_string()},
    })
}

fn publish_mixed(harness: &Harness) -> Result<(), String> {
    publish(
        &harness.sink,
        &[
            Entry::Dir("docs"),
            Entry::File("docs/a.pdf", 100),
            Entry::File("docs/b.mp3", 300),
            Entry::Dir("docs/sub"),
            Entry::File("docs/sub/c.pdf", 50),
            // Equal bytes: the legend order decides, documents before audio.
            Entry::Dir("tie"),
            Entry::File("tie/a.mp3", 40),
            Entry::File("tie/b.pdf", 40),
            Entry::Dir("empty"),
            Entry::File("top.pdf", 5),
        ],
        ResultCompleteness::Complete,
        &[],
    )
}

#[test]
fn folder_rows_and_bars_list_their_types_largest_first() -> TestResult {
    let harness = Harness::product(2000)?;
    publish_mixed(&harness)?;
    let view = harness.open_view()?;
    for (seq, format) in [(1, "treemap"), (2, "bar")] {
        let answer = by_type(&harness, &view, seq, format)?;
        let rows = rows_of(&harness, &answer)?;
        let docs = row(&rows, "docs")?;
        assert_eq!(
            docs["typeSlices"],
            json!([
                slice("type.audio", "exact", 1, 300),
                slice("type.document", "exact", 2, 150),
            ]),
            "{format}"
        );
        assert_eq!(
            row(&rows, "tie")?["typeSlices"],
            json!([
                slice("type.document", "exact", 1, 40),
                slice("type.audio", "exact", 1, 40),
            ])
        );
        // Counted, and no file: an empty list.
        assert_eq!(row(&rows, "empty")?["typeSlices"], json!([]));
        assert!(row(&rows, "top.pdf")?.get("typeSlices").is_none());
        // The bar or rectangle of a folder carries the same list as its row. An empty
        // folder has no area, so no shape.
        for name in ["docs", "tie"] {
            let item = item_no(&rows, name)?;
            assert_eq!(
                shape(&answer, item)?["typeSlices"],
                row(&rows, name)?["typeSlices"],
                "{format} {name}"
            );
        }
    }
    Ok(())
}

#[test]
fn without_the_type_criterion_no_row_or_shape_has_slices() -> TestResult {
    let harness = Harness::product(2000)?;
    publish_mixed(&harness)?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "treemap", 3))?;
    let rows = rows_of(&harness, &answer)?;
    assert!(row(&rows, "docs")?.get("typeSlices").is_none());
    let item = item_no(&rows, "docs")?;
    assert!(shape(&answer, item)?.get("typeSlices").is_none());
    Ok(())
}

#[test]
fn an_unread_folder_has_no_slices_and_one_with_unread_folders_below_has_lower_bounds() -> TestResult
{
    let harness = Harness::product(2000)?;
    publish(
        &harness.sink,
        &[
            Entry::Dir("b"),
            Entry::File("b/fb.pdf", 50),
            Entry::Dir("b/locked"),
            Entry::Dir("denied"),
        ],
        ResultCompleteness::Missing,
        &["denied", "b/locked"],
    )?;
    let view = harness.open_view()?;
    let answer = by_type(&harness, &view, 1, "treemap")?;
    let rows = rows_of(&harness, &answer)?;
    assert_eq!(
        row(&rows, "b")?["typeSlices"],
        json!([slice("type.document", "lower_bound", 1, 50)])
    );
    assert!(row(&rows, "denied")?.get("typeSlices").is_none());
    let b = item_no(&rows, "b")?;
    assert_eq!(
        shape(&answer, b)?["typeSlices"],
        row(&rows, "b")?["typeSlices"]
    );
    Ok(())
}

#[test]
fn an_expanded_folder_shows_its_children_and_they_carry_the_slices() -> TestResult {
    let harness = Harness::product(2000)?;
    publish(
        &harness.sink,
        &[
            Entry::Dir("a"),
            Entry::File("a/fa.pdf", 100),
            Entry::Dir("b"),
            Entry::Dir("b/c"),
            Entry::File("b/c/fc.mp3", 50),
        ],
        ResultCompleteness::Complete,
        &[],
    )?;
    let view = harness.open_view()?;
    let mut body = render_body(&view, 1, "treemap", 3);
    body["colorCriterion"] = json!(policy::COLOR_CRITERION_TYPE);
    body["depthLimit"] = json!(2);
    let answer = harness.ok("viz.render", body)?;
    let root = rows_of(&harness, &answer)?;
    let b = item_no(&root, "b")?;
    let b_shape = shape(&answer, b)?;
    // b draws c, so its rectangle has no strip; its table row still lists its types.
    assert!(b_shape.get("typeSlices").is_none(), "{b_shape}");
    assert_eq!(
        row(&root, "b")?["typeSlices"],
        json!([slice("type.audio", "exact", 1, 50)])
    );
    let expanded = harness.ok(
        "viz.rows",
        json!({
            "modelRef": answer["model"]["modelRef"], "resultRevision": answer["resultRevision"],
            "start": 0, "count": 1000, "parentShapeRef": b_shape["shapeRef"],
        }),
    )?;
    let c_row = row(&expanded, "c")?;
    let c = c_row["itemRef"]["itemNo"].as_u64().ok_or("no item for c")?;
    assert_eq!(
        c_row["typeSlices"],
        json!([slice("type.audio", "exact", 1, 50)])
    );
    assert_eq!(shape(&answer, c)?["typeSlices"], c_row["typeSlices"]);
    Ok(())
}
