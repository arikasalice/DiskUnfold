//! The remainder of the totals on the wire, for the three runtime formats served through
//! the product registration.
//!
//! The remainder is the scope minus the drawn, grouped and filtered-out totals. It is
//! exact only when those four are, names what it holds when it is not zero, and is
//! always accounted for by the counts of the summary.
use du_core_contracts::visualization::model::{REMAINDER_TOO_SMALL, VizModel};
use serde_json::{Value, json};

use super::support::{Harness, Publication, id, render_body};
use super::totals::{count, files_of, render, total};
use super::{TestResult, two_files};

/// The state and the value of the remainder, which every product model carries.
pub(super) fn remainder(model: &Value) -> Result<(String, Option<u64>), String> {
    let quantity = &model["totals"]["remainder"]["quantity"];
    let state = quantity["state"]
        .as_str()
        .ok_or_else(|| format!("no remainder in {}", model["totals"]))?;
    let value = quantity["value"]
        .as_str()
        .and_then(|value| value.parse().ok());
    Ok((state.to_owned(), value))
}

fn exact(value: u64) -> (String, Option<u64>) {
    ("exact".to_owned(), Some(value))
}

/// The cause keys of the remainder; null when the list is omitted.
pub(super) fn keys(model: &Value) -> &Value {
    &model["totals"]["remainderReasonKeys"]
}

/// The model survives the trip over the wire and passes the contract checks.
pub(super) fn assert_valid(model: &Value) -> TestResult {
    let parsed: VizModel = serde_json::from_value(model.clone()).map_err(|e| e.to_string())?;
    parsed.validate_revision(&id(3)).map_err(str::to_owned)
}

/// A remainder that is not zero is made of siblings that the summary counts.
pub(super) fn assert_explained_by_counts(model: &Value) -> TestResult {
    if remainder(model)?.1.is_some_and(|value| value != 0) {
        let counted = count(model, "omittedItems")? + count(model, "unknownItems")?;
        assert!(counted > 0, "{}", model["summary"]);
    }
    Ok(())
}

fn eight_files() -> Vec<(u8, Option<u64>)> {
    (0..8_u8)
        .map(|n| (b'a' + n, Some(10 * (u64::from(n) + 1))))
        .collect()
}

fn render_as(
    harness: &Harness,
    files: &[(u8, Option<u64>)],
    format: &str,
) -> Result<Value, String> {
    harness.publish(&Publication {
        files,
        ..two_files(3)
    })?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, format, 3))?;
    assert_eq!(answer["kind"], "model", "{format}: {answer}");
    Ok(answer)
}

#[test]
fn every_runtime_format_serves_a_model_that_passes_the_contract_checks() -> TestResult {
    let cases = [
        (2000, "treemap"),
        (4, "treemap"),
        (2000, "bar"),
        (4, "bar"),
        (2000, "histogram"),
    ];
    for (limit, format) in cases {
        let harness = Harness::product(limit)?;
        let answer = render_as(&harness, &eight_files(), format)?;
        let model = &answer["model"];
        assert_valid(model)?;
        // Nothing is left out: the three totals are the scope and the list of causes is omitted.
        assert_eq!(remainder(model)?, exact(0), "{format} {limit}");
        assert_eq!(keys(model), &Value::Null, "{format} {limit}");
        assert_eq!(
            total(model, "drawn")? + total(model, "grouped")? + total(model, "filteredOut")?,
            total(model, "scope")?,
            "{format} {limit}"
        );
        assert_explained_by_counts(model)?;
    }
    Ok(())
}

#[test]
fn siblings_below_the_minimum_area_are_the_remainder_of_a_treemap_and_say_so() -> TestResult {
    let harness = Harness::product(4)?;
    // Thirty small files of ten bytes are too small to be drawn, even as a group.
    let files = files_of(&[3_000], 10, 30);
    let answer = render(&harness, &files)?;
    let model = &answer["model"];
    assert_valid(model)?;
    assert_eq!(remainder(model)?, exact(300));
    assert_eq!(keys(model), &json!([REMAINDER_TOO_SMALL]));
    assert_eq!(count(model, "omittedItems")?, 30);
    assert_explained_by_counts(model)
}

#[test]
fn a_bar_below_the_coordinate_precision_is_the_remainder_and_says_so() -> TestResult {
    let harness = Harness::product(2000)?;
    // The 100-byte bar is a twenty-millionth of the axis, below a unit of the grid.
    let files = [(b'a', Some(2_000_000_000)), (b'b', Some(100))];
    let answer = render_as(&harness, &files, "bar")?;
    let model = &answer["model"];
    assert_valid(model)?;
    assert_eq!(model["shapes"].as_array().map(Vec::len), Some(1));
    assert_eq!(total(model, "scope")?, 2_000_000_100);
    assert_eq!(total(model, "drawn")?, 2_000_000_000);
    assert_eq!(remainder(model)?, exact(100));
    assert_eq!(keys(model), &json!([REMAINDER_TOO_SMALL]));
    assert_eq!(count(model, "omittedItems")?, 1);
    assert_explained_by_counts(model)
}

#[test]
fn a_bar_with_a_group_and_a_bar_below_the_precision_counts_each_once() -> TestResult {
    let harness = Harness::product(4)?;
    // Three bars and the group: the 100 and 90 byte bars lack precision, the five smaller
    // files are the group, and nothing is counted twice.
    let files = [
        (b'a', Some(2_000_000_000)),
        (b'b', Some(100)),
        (b'c', Some(90)),
        (b'd', Some(80)),
        (b'e', Some(70)),
        (b'f', Some(60)),
        (b'g', Some(50)),
        (b'h', Some(40)),
    ];
    let answer = render_as(&harness, &files, "bar")?;
    let model = &answer["model"];
    assert_valid(model)?;
    assert_eq!(total(model, "scope")?, 2_000_000_490);
    assert_eq!(total(model, "drawn")?, 2_000_000_000);
    assert_eq!(total(model, "grouped")?, 300);
    assert_eq!(remainder(model)?, exact(190));
    assert_eq!(keys(model), &json!([REMAINDER_TOO_SMALL]));
    assert_eq!(count(model, "omittedItems")?, 2);
    assert_eq!(count(model, "groupedItems")?, 5);
    assert_explained_by_counts(model)
}

#[test]
fn a_histogram_has_no_item_shape_so_its_intervals_are_the_grouped_total() -> TestResult {
    let harness = Harness::product(2000)?;
    let answer = render_as(&harness, &eight_files(), "histogram")?;
    let model = &answer["model"];
    // The items of the intervals used to be in the drawn total too: 360 and 360.
    assert_eq!(total(model, "scope")?, 360);
    assert_eq!(total(model, "drawn")?, 0);
    assert_eq!(total(model, "grouped")?, 360);
    assert_eq!(total(model, "filteredOut")?, 0);
    assert_eq!(remainder(model)?, exact(0));
    let shapes = model["shapes"].as_array().ok_or("no shapes")?;
    assert!(!shapes.is_empty() && shapes.iter().all(|shape| !shape["groupRef"].is_null()));
    Ok(())
}
