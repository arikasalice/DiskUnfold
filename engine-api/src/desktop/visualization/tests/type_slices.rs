//! Type slices on folder rows and shapes (#298, F06 README 54): the files of each
//! type below a folder, from one function for the rows and the shapes.
use super::lower_bound::{Entry, item_no, publish};
use super::support::{Harness, Publication, render_body};
use super::{TestResult, rows_of, two_files};
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

#[test]
fn the_other_group_of_a_bar_chart_lists_its_members_types() -> TestResult {
    // Four shapes at most: the three largest are bars, the rest is the Other group.
    let harness = Harness::product(4)?;
    publish(
        &harness.sink,
        &[
            Entry::Dir("big"),
            Entry::File("big/a.mp4", 900),
            Entry::File("b.iso", 800),
            Entry::File("c.zip", 700),
            Entry::Dir("small"),
            Entry::File("small/d.pdf", 30),
            Entry::File("small/e.mp3", 20),
            Entry::File("f.pdf", 15),
            Entry::File("g.mp3", 10),
        ],
        ResultCompleteness::Complete,
        &[],
    )?;
    let view = harness.open_view()?;
    let answer = by_type(&harness, &view, 1, "bar")?;
    let group = answer["model"]["shapes"]
        .as_array()
        .and_then(|shapes| shapes.iter().find(|shape| !shape["groupRef"].is_null()))
        .ok_or("no group shape")?;
    // The folder member adds the files below it; the file members are their own type.
    let expected = json!([
        slice("type.document", "exact", 2, 45),
        slice("type.audio", "exact", 2, 30),
    ]);
    assert_eq!(group["typeSlices"], expected);
    let rows = rows_of(&harness, &answer)?;
    let row = rows["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| !row["groupRef"].is_null()))
        .ok_or("no group row")?;
    assert_eq!(row["typeSlices"], expected);
    // Without the type criterion the group has none.
    let plain = harness.ok("viz.render", render_body(&view, 3, "bar", 3))?;
    let plain_group = plain["model"]["shapes"]
        .as_array()
        .and_then(|shapes| shapes.iter().find(|shape| !shape["groupRef"].is_null()))
        .ok_or("no group shape")?;
    assert!(plain_group.get("typeSlices").is_none());
    Ok(())
}

#[test]
fn the_other_group_of_a_treemap_lists_its_members_types() -> TestResult {
    let harness = Harness::product(4)?;
    // As in the groups test: two large files and forty small ones cannot all keep a
    // shape within four, so the small ones merge into the Other group.
    let mut named: Vec<(&'static [&'static [u8]], Option<u64>)> =
        vec![(&[b"a.mp4"], Some(1_200)), (&[b"b.iso"], Some(1_200))];
    for index in 0..40_usize {
        let extension = if index % 2 == 0 { "pdf" } else { "mp3" };
        let name: &'static [u8] = format!("m{index}.{extension}").into_bytes().leak();
        let path: &'static [&'static [u8]] = vec![name].leak();
        named.push((path, Some(40)));
    }
    harness.publish(&Publication {
        files: &[],
        named: &named,
        ..two_files(3)
    })?;
    let view = harness.open_view()?;
    let answer = by_type(&harness, &view, 1, "treemap")?;
    let group = answer["model"]["shapes"]
        .as_array()
        .and_then(|shapes| shapes.iter().find(|shape| !shape["groupRef"].is_null()))
        .ok_or("no group shape")?;
    // Equal bytes: the legend order puts documents first.
    let expected = json!([
        slice("type.document", "exact", 20, 800),
        slice("type.audio", "exact", 20, 800),
    ]);
    assert_eq!(group["typeSlices"], expected);
    let rows = rows_of(&harness, &answer)?;
    let row = rows["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| !row["groupRef"].is_null()))
        .ok_or("no group row")?;
    assert_eq!(row["typeSlices"], expected);
    Ok(())
}
