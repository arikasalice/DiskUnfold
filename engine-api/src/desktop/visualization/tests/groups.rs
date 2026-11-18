//! The Other group of a treemap that is served through the product registration.
use serde_json::json;

use super::support::{Harness, Publication, render_body};
use super::{TestResult, assert_unwatched, rows_of, two_files};
use crate::viz_production::policy;

#[test]
fn treemap_over_the_shape_limit_serves_the_other_group_with_its_label_key() -> TestResult {
    let harness = Harness::product(4)?;
    // Two large files and forty small ones cannot all keep a shape within four, so
    // the small ones merge into one group instead of failing the render.
    let mut files: Vec<(u8, Option<u64>)> = vec![(b'a', Some(1_200)), (b'b', Some(1_200))];
    files.extend(
        (b'c'..=b'z')
            .chain(b'A'..=b'P')
            .map(|name| (name, Some(40))),
    );
    assert_eq!(files.len(), 42);
    harness.publish(&Publication {
        files: &files,
        ..two_files(3)
    })?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "treemap", 3))?;
    assert_eq!(answer["kind"], "model", "{answer}");
    assert_unwatched(&answer);
    let shapes = answer["model"]["shapes"].as_array().ok_or("no shapes")?;
    assert!(shapes.len() <= 4, "{} shapes", shapes.len());
    let group = shapes
        .iter()
        .find(|shape| !shape["groupRef"].is_null())
        .ok_or("no group shape")?;
    assert_eq!(group["displayFlags"], json!([policy::FLAG_OTHER_GROUP]));
    assert_eq!(answer["model"]["summary"]["groupedItems"]["value"], "40");
    // The group row stands in for its members: two item rows and the group row.
    let rows = rows_of(&harness, &answer)?;
    let rows = rows["rows"].as_array().ok_or("no rows")?;
    assert_eq!(rows.len(), 3);
    let other = rows
        .iter()
        .find(|row| !row["groupRef"].is_null())
        .ok_or("no group row")?;
    assert_eq!(other["groupRef"], group["groupRef"]);
    assert_eq!(other["displayName"], policy::GROUP_OTHER_NAME_KEY);
    assert_eq!(other["labelKey"], "ui.visualization.group.other");
    assert_eq!(other["labelKey"], other["displayName"]);
    assert_eq!(other["legendClassKey"], policy::LEGEND_CLASS_UNCLASSIFIED);
    for row in rows.iter().filter(|row| row["groupRef"].is_null()) {
        assert!(row.get("labelKey").is_none(), "{row}");
    }
    let members = harness.ok(
        "viz.group_rows",
        json!({
            "modelRef": answer["model"]["modelRef"], "groupRef": group["groupRef"],
            "start": 0, "count": 1000,
        }),
    )?;
    assert_eq!(members["totalCount"], 40);
    assert!(
        members["rows"]
            .as_array()
            .ok_or("no member rows")?
            .iter()
            .all(|row| row.get("labelKey").is_none() && row["groupRef"].is_null())
    );
    Ok(())
}
