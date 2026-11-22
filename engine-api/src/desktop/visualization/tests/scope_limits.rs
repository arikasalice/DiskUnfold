//! The scope total of a model carries F03's measurement limits (#351): the same keys
//! `space.total` gives the same item in the same size basis.
use super::support::{Harness, render_body};
use super::{TestResult, two_files};
use serde_json::{Value, json};

fn limits_of_space_total(harness: &Harness, basis: &str) -> Result<Value, String> {
    let total = harness.ok(
        "space.total",
        json!({
            "requestSeq": 1,
            "resultVersion": crate::desktop::visualization::tests::support::id(3),
            "target": {"kind": "folder", "item": {"resultVersion": crate::desktop::visualization::tests::support::id(3), "itemNo": 0}},
        }),
    )?;
    total["sizes"]
        .as_array()
        .and_then(|sizes| sizes.iter().find(|size| size["size"]["basis"] == basis))
        .map(|size| size["measurementLimits"].clone())
        .ok_or_else(|| format!("no {basis} size in {total}"))
}

#[test]
fn the_scope_total_carries_the_limits_space_total_gives_the_same_item() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&two_files(3))?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "treemap", 3))?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let limits = &answer["model"]["totals"]["scopeMeasurementLimits"];
    let expected = limits_of_space_total(&harness, "logical")?;
    // The fixture's logical total has limits, so the two lists are compared, not omitted.
    assert!(
        expected.as_array().is_some_and(|keys| !keys.is_empty()),
        "{expected}"
    );
    assert_eq!(limits, &expected);
    // The shared-extent key belongs to the allocated basis only (F03 measurement_limits).
    assert!(
        !limits.as_array().is_some_and(|keys| keys
            .iter()
            .any(|key| key == "space.allocated_may_include_shared")),
        "{limits}"
    );
    Ok(())
}
