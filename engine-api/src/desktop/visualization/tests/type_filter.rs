//! Narrowing to one type from the legend through the desktop assembly (F06 FR14, #301).
use serde_json::{Value, json};

use super::support::{Harness, Publication, render_body};
use super::{TestResult, rows_of, two_files};
use crate::viz_production::policy;

/// 220 bytes: 100 of video in clip.mp4 and docs/talk.mp4, 120 of other types.
fn mixed() -> Publication<'static> {
    Publication {
        files: &[],
        named: &[
            (&[b"clip.mp4"], Some(60)),
            (&[b"docs", b"report.pdf"], Some(100)),
            (&[b"docs", b"talk.mp4"], Some(40)),
            (&[b"songs", b"x.mp3"], Some(20)),
        ],
        ..two_files(3)
    }
}

fn typed(view: &Value, seq: u32, format: &str) -> Value {
    let mut body = render_body(view, seq, format, 3);
    body["colorCriterion"] = json!(policy::COLOR_CRITERION_TYPE);
    body
}

fn legend_entry<'a>(model: &'a Value, key: &str) -> Result<&'a Value, String> {
    model["legend"]
        .as_array()
        .ok_or("no legend")?
        .iter()
        .find(|entry| entry["key"] == key)
        .ok_or_else(|| format!("no legend entry {key}"))
}

fn names(rows: &Value) -> Result<Vec<String>, String> {
    rows["rows"]
        .as_array()
        .ok_or("no rows")?
        .iter()
        .map(|row| Ok(row["displayName"].as_str().ok_or("no name")?.to_owned()))
        .collect()
}

fn bytes(total: &Value) -> &Value {
    &total["quantity"]["value"]
}

/// The render body narrowed by the condition of one legend row of the whole scope.
fn narrowed(harness: &Harness, view: &Value, key: &str, format: &str) -> Result<Value, String> {
    let whole = harness.ok("viz.render", typed(view, 1, "treemap"))?;
    let entry = legend_entry(&whole["model"], key)?;
    let mut body = typed(view, 2, format);
    body["condition"] = json!({"kind": "normalized", "condition": entry["condition"]});
    Ok(body)
}

fn type_condition(operand: &[u8]) -> Value {
    json!({"kind": "normalized", "condition": {"kind": "predicate", "predicate": {
        "kind": "registered", "id": "pred.file_type", "revision": 1,
        "operand": du_core_contracts::PathBytes::from_bytes(operand),
    }}})
}

#[test]
fn a_legend_row_narrows_the_treemap_to_its_type_and_keeps_the_scope_legend() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&mixed())?;
    let view = harness.open_view()?;
    let whole = harness.ok("viz.render", typed(&view, 1, "treemap"))?;
    let video = legend_entry(&whole["model"], "type.video")?;
    assert!(video["condition"].is_object(), "{video}");
    let mut body = typed(&view, 2, "treemap");
    body["condition"] = json!({"kind": "normalized", "condition": video["condition"]});
    let narrowed = harness.ok("viz.render", body)?;
    assert_eq!(narrowed["kind"], "model", "{narrowed}");
    let model = &narrowed["model"];
    let mut shown = names(&rows_of(&harness, &narrowed)?)?;
    shown.sort();
    assert_eq!(shown, ["clip.mp4", "docs"]);
    // Drawn holds the kept bytes; the scope keeps its value; the rest is filtered out.
    let totals = &model["totals"];
    assert_eq!(bytes(&totals["scope"]), "220");
    assert_eq!(bytes(&totals["drawn"]), "100");
    assert_eq!(bytes(&totals["filteredOut"]), "120");
    assert_eq!(bytes(&totals["remainder"]), "0");
    // The legend keeps the whole scope's values to compare against.
    assert_eq!(legend_entry(model, "type.video")?["total"], video["total"]);
    // Its video row counts the videos in folders too, so it says what narrowing draws.
    assert_eq!(bytes(&video["total"]), "100");
    assert_eq!(video["itemCount"]["value"], "2");
    assert_ne!(
        model["conditionFingerprint"],
        whole["model"]["conditionFingerprint"]
    );
    Ok(())
}

#[test]
fn an_expanded_folder_shows_only_its_files_of_the_type() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&mixed())?;
    let view = harness.open_view()?;
    let mut body = narrowed(&harness, &view, "type.video", "treemap")?;
    body["depthLimit"] = json!(2);
    let answer = harness.ok("viz.render", body)?;
    let shapes = answer["model"]["shapes"].as_array().ok_or("no shapes")?;
    // clip.mp4, docs and docs/talk.mp4; report.pdf, songs and x.mp3 have no shape.
    assert_eq!(shapes.len(), 3, "{shapes:?}");
    assert!(
        shapes
            .iter()
            .all(|shape| shape["legendClassKey"] != "type.document")
    );
    Ok(())
}

#[test]
fn a_type_the_scope_does_not_hold_is_an_empty_model_and_not_an_error() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&mixed())?;
    let view = harness.open_view()?;
    let mut body = typed(&view, 1, "treemap");
    body["condition"] = type_condition(b"type.archive");
    let answer = harness.ok("viz.render", body)?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let model = &answer["model"];
    assert_eq!(model["shapes"], json!([]));
    assert_eq!(bytes(&model["totals"]["drawn"]), "0");
    assert_eq!(bytes(&model["totals"]["filteredOut"]), "220");
    Ok(())
}

#[test]
fn a_format_that_does_not_narrow_and_a_foreign_operand_are_refused() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&mixed())?;
    let view = harness.open_view()?;
    let condition = narrowed(&harness, &view, "type.video", "treemap")?["condition"].clone();
    // The histogram groups by size, not by folder: it does not narrow.
    let mut body = typed(&view, 3, "histogram");
    body["condition"] = condition;
    assert_eq!(harness.reason("viz.render", body)?, "predicate.not_allowed");
    for (seq, operand) in [
        (5, b"unclassified".as_slice()),
        (6, b"type.folder"),
        (7, b"video"),
    ] {
        let mut body = typed(&view, seq, "treemap");
        body["condition"] = type_condition(operand);
        assert_eq!(harness.reason("viz.render", body)?, "query.syntax_invalid");
    }
    Ok(())
}

#[test]
fn a_legend_row_narrows_the_bar_chart_to_its_type() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&mixed())?;
    let view = harness.open_view()?;
    let body = narrowed(&harness, &view, "type.video", "bar")?;
    let answer = harness.ok("viz.render", body)?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let mut shown = names(&rows_of(&harness, &answer)?)?;
    shown.sort();
    // clip.mp4 by its 60 bytes and docs by talk.mp4's 40; songs keeps no video.
    assert_eq!(shown, ["clip.mp4", "docs"]);
    // Narrowed, the folder holds only videos and takes their class (README 46.12).
    let rows = rows_of(&harness, &answer)?;
    let classes: Vec<_> = rows["rows"]
        .as_array()
        .ok_or("no rows")?
        .iter()
        .map(|row| row["legendClassKey"].clone())
        .collect();
    assert_eq!(classes, [json!("type.video"), json!("type.video")]);
    let totals = &answer["model"]["totals"];
    assert_eq!(bytes(&totals["scope"]), "220");
    assert_eq!(bytes(&totals["drawn"]), "100");
    assert_eq!(bytes(&totals["filteredOut"]), "120");
    assert_eq!(bytes(&totals["remainder"]), "0");
    // The legend keeps the whole scope's values, as on the treemap.
    assert_eq!(
        bytes(&legend_entry(&answer["model"], "type.video")?["total"]),
        "100"
    );
    Ok(())
}

/// 25 videos of 100..=124 bytes and one 1,000-byte document, all directly in the scope.
fn many_videos() -> Publication<'static> {
    let mut named: Vec<(&'static [&'static [u8]], Option<u64>)> = (0..25_u64)
        .map(|index| {
            let name: &'static [u8] = Box::leak(
                format!("clip-{index:02}.mp4")
                    .into_bytes()
                    .into_boxed_slice(),
            );
            let path: &'static [&'static [u8]] = Box::leak(vec![name].into_boxed_slice());
            (path, Some(100 + index))
        })
        .collect();
    named.push((&[b"report.pdf"], Some(1_000)));
    Publication {
        files: &[],
        named: Box::leak(named.into_boxed_slice()),
        ..two_files(3)
    }
}

#[test]
fn a_narrowed_bar_chart_groups_past_twenty_bars_in_the_types_class() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&many_videos())?;
    let view = harness.open_view()?;
    let body = narrowed(&harness, &view, "type.video", "bar")?;
    let answer = harness.ok("viz.render", body)?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let model = &answer["model"];
    let shapes = model["shapes"].as_array().ok_or("no shapes")?;
    // The largest 20 videos, then one Other of the other 5 (README 47).
    assert_eq!(shapes.len(), 21);
    let group = shapes
        .iter()
        .find(|shape| shape["groupRef"].is_string())
        .ok_or("no group")?;
    assert_eq!(group["legendClassKey"], "type.video");
    assert_eq!(group["memberCount"]["value"], "5");
    let totals = &model["totals"];
    assert_eq!(bytes(&totals["scope"]), "3800");
    assert_eq!(bytes(&totals["drawn"]), "2290");
    assert_eq!(bytes(&totals["grouped"]), "510");
    assert_eq!(bytes(&totals["filteredOut"]), "1000");
    assert_eq!(bytes(&totals["remainder"]), "0");
    Ok(())
}
