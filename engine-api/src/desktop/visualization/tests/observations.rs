//! F06 #111 characterization through registered commands, not a public contract.
use serde_json::json;

use super::support::{Harness, Publication, render_body};
use super::{TestResult, rows_of, two_files};

#[test]
fn records_current_empty_histogram_bins_across_rows_and_group_pages() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&Publication {
        files: &[(b'a', Some(1))],
        ..two_files(3)
    })?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "histogram", 3))?;
    assert_eq!(answer["kind"], "model", "{answer}");

    let model = &answer["model"];
    let rows = rows_of(&harness, &answer)?;
    let row_list = rows["rows"].as_array().ok_or("no histogram rows")?;
    let empty = row_list
        .iter()
        .find(|row| row["quantity"]["value"] == "0")
        .ok_or("no empty histogram bin row")?;
    let group_ref = empty["groupRef"]
        .as_str()
        .ok_or("empty bin row has no group reference")?;
    assert!(
        model["shapes"].as_array().is_some_and(|shapes| {
            shapes
                .iter()
                .all(|shape| shape["groupRef"].as_str() != Some(group_ref))
        }),
        "an empty bin unexpectedly has a shape: {}",
        model["shapes"]
    );

    let page = harness.ok(
        "viz.group_rows",
        json!({
            "modelRef": model["modelRef"], "groupRef": group_ref,
            "start": 0, "count": 1000,
        }),
    )?;
    assert_eq!(page["totalCount"], 0);
    assert_eq!(page["rows"], json!([]));
    assert_eq!(page["groupTotal"]["bytes"], "0");
    assert_eq!(model["summary"]["groupedItems"]["value"], "1");
    assert_eq!(model["summary"]["unknownItems"]["value"], "0");
    Ok(())
}

#[test]
fn records_current_zero_size_other_tail_without_a_shape() -> TestResult {
    let harness = Harness::product(3)?;
    let files = [
        (b'a', Some(100)),
        (b'b', Some(0)),
        (b'c', Some(0)),
        (b'd', Some(0)),
    ];
    harness.publish(&Publication {
        files: &files,
        ..two_files(3)
    })?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "bar", 3))?;
    assert_eq!(answer["kind"], "model", "{answer}");

    let model = &answer["model"];
    let rows = rows_of(&harness, &answer)?;
    let row_list = rows["rows"].as_array().ok_or("no ranked rows")?;
    let group = row_list
        .iter()
        .find(|row| !row["groupRef"].is_null())
        .ok_or("no Other group row")?;
    let group_ref = group["groupRef"]
        .as_str()
        .ok_or("Other group row has no reference")?;
    assert_eq!(group["size"]["bytes"], "0");
    assert!(
        model["shapes"].as_array().is_some_and(|shapes| {
            shapes
                .iter()
                .all(|shape| shape["groupRef"].as_str() != Some(group_ref))
        }),
        "zero-size Other unexpectedly has a shape: {}",
        model["shapes"]
    );

    let page = harness.ok(
        "viz.group_rows",
        json!({
            "modelRef": model["modelRef"], "groupRef": group_ref,
            "start": 0, "count": 1000,
        }),
    )?;
    let members = page["rows"].as_array().ok_or("no Other member rows")?;
    assert_eq!(page["totalCount"], 2);
    assert_eq!(members.len(), 2);
    assert_eq!(page["groupTotal"]["bytes"], "0");
    assert!(members.iter().all(|row| row["size"]["bytes"] == "0"));
    assert_eq!(model["summary"]["groupedItems"]["value"], "2");
    Ok(())
}
