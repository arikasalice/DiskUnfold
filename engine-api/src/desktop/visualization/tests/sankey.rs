//! The Sankey of a scope (#296, F06 README 12.1): scope → folders → types, conserved in
//! every column, with the same link bytes on the relationships and the node rows.
use super::lower_bound::{Entry, publish};
use super::support::{Harness, render_body};
use super::{TestResult, rows_of};
use du_core_contracts::result::ResultCompleteness;
use serde_json::{Value, json};
use std::collections::HashMap;

fn sankey(harness: &Harness, view: &Value, seq: u32) -> Result<Value, String> {
    harness.ok("viz.render", render_body(view, seq, "sankey", 3))
}

fn publish_mixed(
    harness: &Harness,
    completeness: ResultCompleteness,
    gaps: &[&str],
) -> Result<(), String> {
    publish(
        &harness.sink,
        &[
            Entry::Dir("work"),
            Entry::File("work/a.pdf", 100),
            Entry::File("work/b.mp4", 300),
            Entry::Dir("photos"),
            Entry::File("photos/c.jpg", 200),
            Entry::File("top.mp3", 50),
        ],
        completeness,
        gaps,
    )
}

fn bytes(value: &Value) -> u64 {
    value["value"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .unwrap_or(u64::MAX)
}

fn shape_bytes(shape: &Value) -> u64 {
    shape["size"]["bytes"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .unwrap_or(u64::MAX)
}

#[test]
fn every_column_adds_up_to_the_scope_and_rows_list_the_same_links() -> TestResult {
    let harness = Harness::product(2000)?;
    publish_mixed(&harness, ResultCompleteness::Complete, &[])?;
    let view = harness.open_view()?;
    let answer = sankey(&harness, &view, 1)?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let model = &answer["model"];
    let shapes = model["shapes"].as_array().ok_or("no shapes")?;
    let mut columns: HashMap<String, u64> = HashMap::new();
    for shape in shapes {
        let column = shape["column"].as_str().ok_or("a shape without a column")?;
        *columns.entry(column.to_owned()).or_default() += shape_bytes(shape);
    }
    assert_eq!(columns["scope"], 650);
    assert_eq!(columns["folder"], 650);
    assert_eq!(columns["type"], 650);
    // Folder → type links, keyed by the shape references they join.
    let refs: HashMap<&str, &Value> = shapes
        .iter()
        .filter_map(|shape| Some((shape["shapeRef"].as_str()?, shape)))
        .collect();
    let mut by_relation = HashMap::new();
    for link in model["relationships"].as_array().ok_or("no links")? {
        assert_eq!(link["kind"], "viz.sankey.link");
        assert_eq!(link["quantity"]["state"], "exact");
        let from = refs[link["fromShapeRef"].as_str().ok_or("from")?];
        let to = refs[link["toShapeRef"].as_str().ok_or("to")?];
        let order = |column: &Value| ["scope", "folder", "type"].iter().position(|c| column == c);
        assert_eq!(
            order(&from["column"]).map(|n| n + 1),
            order(&to["column"]),
            "{link}"
        );
        if from["column"] == "folder" {
            by_relation.insert((key(from), key(to)), bytes(&link["quantity"]));
        }
    }
    // The rows list the same folder → type links, with the same bytes.
    let rows = rows_of(&harness, &answer)?;
    let rows = rows["rows"].as_array().ok_or("no rows")?;
    let mut by_row = HashMap::new();
    for row in rows {
        for link in row["links"].as_array().into_iter().flatten() {
            by_row.insert(
                (key(row), link["toRowRef"].as_str().unwrap_or("").to_owned()),
                bytes(&link["quantity"]),
            );
        }
    }
    assert_eq!(by_row.len(), by_relation.len());
    for (pair, value) in &by_relation {
        assert_eq!(by_row.get(pair), Some(value), "{pair:?}");
    }
    // Rows go column by column: the scope, then the folders, then the types.
    assert!(rows[0]["groupRef"].is_null());
    let types = rows
        .iter()
        .filter(|row| {
            row["groupRef"]
                .as_str()
                .is_some_and(|g| g.starts_with("viz.sankey.type."))
        })
        .count();
    assert!(rows[rows.len() - types..].iter().all(|row| {
        row["groupRef"]
            .as_str()
            .is_some_and(|g| g.starts_with("viz.sankey.type."))
    }));
    Ok(())
}

/// The row reference a link names: `item:<n>` or `group:<ref>`.
fn key(value: &Value) -> String {
    match value["itemRef"]["itemNo"].as_u64() {
        Some(item) => format!("item:{item}"),
        None => format!("group:{}", value["groupRef"].as_str().unwrap_or("")),
    }
}

#[test]
fn a_scope_that_is_only_a_lower_bound_shows_the_table_instead() -> TestResult {
    let harness = Harness::product(2000)?;
    publish_mixed(&harness, ResultCompleteness::Missing, &["photos"])?;
    let view = harness.open_view()?;
    let answer = sankey(&harness, &view, 1)?;
    assert_eq!(answer["kind"], "unavailable", "{answer}");
    assert_eq!(answer["reasonKey"], "viz.unavailable.no_attribution");
    assert!(
        rows_of(&harness, &answer)?["totalCount"]
            .as_u64()
            .is_some_and(|n| n > 0)
    );
    Ok(())
}

#[test]
fn the_type_column_does_not_depend_on_the_colour_criterion() -> TestResult {
    let harness = Harness::product(2000)?;
    publish_mixed(&harness, ResultCompleteness::Complete, &[])?;
    let view = harness.open_view()?;
    // The request body uses the "none" criterion; the right column still holds types.
    let answer = sankey(&harness, &view, 1)?;
    let types: Vec<_> = answer["model"]["shapes"]
        .as_array()
        .ok_or("no shapes")?
        .iter()
        .filter(|shape| shape["column"] == "type")
        .map(|shape| shape["legendClassKey"].clone())
        .collect();
    assert_eq!(
        types,
        [
            json!("type.document"),
            json!("type.audio"),
            json!("type.image"),
            json!("type.video")
        ]
    );
    Ok(())
}

#[test]
fn a_shape_limit_too_small_for_one_folder_node_shows_the_table_and_one_more_keeps_only_other()
-> TestResult {
    // Scope, nine types and two unattributed nodes: twelve before any folder node.
    let small = Harness::product(12)?;
    publish_mixed(&small, ResultCompleteness::Complete, &[])?;
    let view = small.open_view()?;
    let refused = sankey(&small, &view, 1)?;
    assert_eq!(refused["kind"], "unavailable", "{refused}");
    let one = Harness::product(13)?;
    publish_mixed(&one, ResultCompleteness::Complete, &[])?;
    let view = one.open_view()?;
    let answer = sankey(&one, &view, 1)?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let folders: Vec<_> = answer["model"]["shapes"]
        .as_array()
        .ok_or("no shapes")?
        .iter()
        .filter(|shape| shape["column"] == "folder")
        .collect();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0]["groupRef"], "viz.sankey.other");
    assert_eq!(folders[0]["memberCount"]["value"], "3");
    let members = one.ok(
        "viz.group_rows",
        json!({"modelRef": answer["model"]["modelRef"], "groupRef": "viz.sankey.other", "start": 0, "count": 10}),
    )?;
    assert_eq!(members["totalCount"], 3);
    Ok(())
}
