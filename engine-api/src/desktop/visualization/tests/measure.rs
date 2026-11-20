//! The histogram's two vertical axes: bytes for the size measure, items for the count measure.
use du_core_contracts::ErrorCode;
use serde_json::{Value, json};

use super::support::{Harness, render_body};
use super::{TestResult, rows_of, two_files};
use crate::viz_production::policy;

fn interval_units(rows: &Value) -> Result<Vec<String>, String> {
    Ok(rows["rows"]
        .as_array()
        .ok_or("no rows")?
        .iter()
        .filter(|row| !row["groupRef"].is_null())
        .map(|row| {
            row["quantity"]["unit"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect())
}

#[test]
fn a_histogram_counts_items_for_the_count_measure_and_bytes_for_size() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&two_files(3))?;
    let view = harness.open_view()?;
    for (seq, (measure, unit)) in (1..).zip([
        (policy::MEASURE_SIZE, "bytes"),
        (policy::MEASURE_COUNT, "items"),
    ]) {
        let mut body = render_body(&view, seq, "histogram", 3);
        body["measure"] = json!(measure);
        let answer = harness.ok("viz.render", body)?;
        assert_eq!(answer["kind"], "model", "{answer}");
        assert_eq!(answer["model"]["representation"]["measure"], measure);
        let rows = rows_of(&harness, &answer)?;
        let units = interval_units(&rows)?;
        assert!(!units.is_empty(), "{rows}");
        assert!(units.iter().all(|u| u == unit), "{measure}: {units:?}");
        assert_eq!(rows["rows"][0]["denominator"]["value"]["unit"], unit);
    }
    Ok(())
}

#[test]
fn only_the_histogram_serves_the_count_measure() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&two_files(3))?;
    let view = harness.open_view()?;
    for (seq, format) in (1..).zip(["treemap", "bar"]) {
        let mut body = render_body(&view, seq, format, 3);
        body["measure"] = json!(policy::MEASURE_COUNT);
        let error = harness
            .call("viz.render", body)
            .error
            .ok_or("a count render of another format succeeded")?;
        assert_eq!(error.reason_key, "viz.measure_unknown", "{format}");
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{format}");
    }
    Ok(())
}
