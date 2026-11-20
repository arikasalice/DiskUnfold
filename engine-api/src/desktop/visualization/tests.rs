//! The default registration, served through the desktop assembly and a real result store.
mod classification_criterion;
mod folders;
mod groups;
mod lower_bound;
mod measure;
mod remainder;
mod revisions;
mod support;
mod totals;

use std::sync::{Arc, atomic::Ordering};

use du_core_contracts::{ErrorCode, size_units::SI_UNITS};
use serde_json::{Value, json};

use self::support::{Harness, Ports, Publication, id, render_body, view_policy};
use super::DefaultVisualization;
use crate::{api::EngineBuilder, desktop::SharedTableLimit, viz_production::policy};

type TestResult = Result<(), String>;

const COMMANDS: [&str; 9] = [
    "viz.view.open",
    "viz.view.close",
    "viz.render",
    "viz.cancel",
    "viz.rows",
    "viz.group_rows",
    "viz.locate",
    "viz.catalog",
    "viz.availability",
];

fn two_files(revision: u8) -> Publication<'static> {
    Publication {
        fingerprint: "scope-a",
        lineage_candidate: 1,
        scope: revision - 1,
        revision,
        files: &[(b'a', Some(25)), (b'b', Some(75))],
    }
}

fn scope(revision: u8) -> Value {
    json!({"resultVersion": id(revision), "itemNo": 0})
}

fn availability(harness: &Harness, lineage: &Value, revision: u8) -> Result<Value, String> {
    harness.ok(
        "viz.availability",
        json!({
            "sourceRef": {"kind": "scan", "ref": lineage},
            "resultRevision": id(revision),
            "aggregateScopeRef": scope(revision),
        }),
    )
}

fn availability_of(answer: &Value, format: &str) -> Result<Value, String> {
    answer["formats"]
        .as_array()
        .and_then(|formats| formats.iter().find(|entry| entry["format"] == format))
        .cloned()
        .ok_or_else(|| format!("no availability entry for {format}"))
}

/// Reads the rows of the model or fallback table a render answer refers to.
fn rows_of(harness: &Harness, answer: &Value) -> Result<Value, String> {
    let (model_ref, revision) = if answer["kind"] == "model" {
        (&answer["model"]["modelRef"], &answer["resultRevision"])
    } else {
        (
            &answer["fallback"]["modelRef"],
            &answer["fallback"]["resultRevision"],
        )
    };
    harness.ok(
        "viz.rows",
        json!({"modelRef": model_ref, "resultRevision": revision, "start": 0, "count": 1000}),
    )
}

/// Models never claim freshness: nothing watches the result, so the state is unknown.
fn assert_unwatched(answer: &Value) {
    let staleness = &answer["model"]["staleness"];
    assert_ne!(staleness["state"], "current", "{staleness}");
    assert_eq!(staleness["state"], "unknown");
    assert_eq!(staleness["reasonKey"], policy::STALENESS_UNWATCHED_REASON);
}

fn row_bytes(rows: &Value) -> Result<Vec<u64>, String> {
    let rows = rows["rows"].as_array().ok_or("rows are not a list")?;
    rows.iter()
        .map(|row| {
            row["size"]["bytes"]
                .as_str()
                .and_then(|bytes| bytes.parse().ok())
                .ok_or_else(|| format!("row without a byte value: {row}"))
        })
        .collect()
}

#[test]
fn default_registration_serves_the_catalog_and_every_command() -> TestResult {
    let harness = Harness::product(2000)?;
    for name in COMMANDS {
        assert!(harness.engine.has_use_case(name), "{name}");
    }
    let catalog = harness.ok("viz.catalog", json!({}))?;
    assert_eq!(
        catalog["formats"],
        json!([
            "bar",
            "histogram",
            "sankey",
            "treemap",
            "trend",
            "waterfall"
        ])
    );
    let strategies: Vec<_> = catalog["strategies"]
        .as_array()
        .ok_or("strategies are not a list")?
        .iter()
        .map(|entry| {
            (
                entry["id"].as_str().unwrap_or_default(),
                entry["revision"].as_u64(),
            )
        })
        .collect();
    assert_eq!(
        strategies,
        [
            ("viz.bar", Some(1)),
            ("viz.histogram", Some(1)),
            ("viz.sankey", Some(1)),
            ("viz.treemap", Some(1)),
            ("viz.trend", Some(1)),
            ("viz.waterfall", Some(1)),
        ]
    );
    assert_eq!(
        catalog["measures"],
        json!([policy::MEASURE_SIZE, policy::MEASURE_COUNT])
    );
    assert_eq!(
        catalog["colorCriteria"],
        json!([policy::COLOR_CRITERION_NONE])
    );
    assert_eq!(
        catalog["denominatorCriteria"],
        json!([policy::DENOMINATOR_SCOPE])
    );
    assert_eq!(catalog["requiredDataTableRevision"], json!(1));
    // A render that finishes proves the viz.compute job kind is registered and runs.
    harness.publish(&two_files(3))?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "treemap", 3))?;
    assert_eq!(answer["kind"], "model");
    Ok(())
}

#[test]
fn a_second_default_registration_is_rejected() -> TestResult {
    let mut builder = EngineBuilder::default();
    let desktop = crate::desktop::register_services(
        &mut builder,
        Arc::new(Ports),
        Arc::new(Ports),
        Arc::new(Ports),
        &Ports,
        Arc::new(SharedTableLimit::default),
    )
    .map_err(|error| error.to_string())?;
    let options = || DefaultVisualization {
        policy: view_policy(2000),
        model_capacity: std::num::NonZeroUsize::MIN,
        unit_table: Arc::new(|| &SI_UNITS),
    };
    let services = &desktop.visualization;
    assert_eq!(
        services.register_default(&mut builder, &Ports, options()),
        Ok(())
    );
    assert_eq!(
        services.register_default(&mut builder, &Ports, options()),
        Err("visualization job registration failed")
    );
    Ok(())
}

#[test]
fn trend_waterfall_and_sankey_are_unavailable_with_the_same_rows_everywhere() -> TestResult {
    let harness = Harness::product(2000)?;
    let lineage =
        serde_json::to_value(harness.publish(&two_files(3))?).map_err(|e| e.to_string())?;
    let answer = availability(&harness, &lineage, 3)?;
    let view = harness.open_view()?;
    let cases = [
        ("trend", "viz.unavailable.single_observation"),
        ("waterfall", "viz.unavailable.no_baseline"),
        ("sankey", "viz.unavailable.no_attribution"),
    ];
    for (seq, (format, reason)) in (1..).zip(cases) {
        let entry = availability_of(&answer, format)?;
        assert_eq!(entry["available"], false, "{format}");
        assert_eq!(entry["reasonKey"], reason, "{format}");
        let rendered = harness.ok("viz.render", render_body(&view, seq, format, 3))?;
        assert_eq!(rendered["kind"], "unavailable", "{format}");
        assert_eq!(rendered["reasonKey"], reason, "{format}");
        assert_eq!(
            rendered["availableFormats"],
            json!(["bar", "histogram", "treemap"])
        );
        assert!(rendered.get("model").is_none(), "{format}");
        let rows = rows_of(&harness, &rendered)?;
        assert_eq!(rows["totalCount"], 2, "{format}");
        assert_eq!(row_bytes(&rows)?.iter().sum::<u64>(), 100, "{format}");
    }
    Ok(())
}

#[test]
fn formats_without_a_compute_route_never_enter_the_treemap_projection() -> TestResult {
    let harness = Harness::overclaiming(2000)?;
    harness.publish(&two_files(3))?;
    let view = harness.open_view()?;
    for (seq, format) in (1..).zip(["trend", "waterfall", "sankey"]) {
        let reason = harness.reason("viz.render", render_body(&view, seq, format, 3))?;
        assert_eq!(reason, "support.internal_error", "{format}");
    }
    assert_eq!(harness.children_read.load(Ordering::SeqCst), 0);
    // The same producer still serves the treemap, which does have a route.
    let treemap = harness.ok("viz.render", render_body(&view, 4, "treemap", 3))?;
    assert_eq!(treemap["kind"], "model");
    assert_eq!(harness.children_read.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn availability_and_render_agree_for_each_catalog_format() -> TestResult {
    let harness = Harness::product(2000)?;
    let lineage =
        serde_json::to_value(harness.publish(&two_files(3))?).map_err(|e| e.to_string())?;
    let answer = availability(&harness, &lineage, 3)?;
    // Without a source lineage the named revision alone is pinned, with the same answer (#241).
    let unnamed = harness.ok(
        "viz.availability",
        json!({"resultRevision": id(3), "aggregateScopeRef": scope(3)}),
    )?;
    assert_eq!(unnamed, answer);
    let view = harness.open_view()?;
    let formats = [
        "treemap",
        "bar",
        "trend",
        "waterfall",
        "histogram",
        "sankey",
    ];
    for (seq, format) in (1..).zip(formats) {
        let entry = availability_of(&answer, format)?;
        let rendered = harness.ok("viz.render", render_body(&view, seq, format, 3))?;
        if entry["available"] == true {
            assert_eq!(rendered["kind"], "model", "{format}: {rendered}");
        } else {
            assert_eq!(rendered["kind"], "unavailable", "{format}");
            assert_eq!(rendered["reasonKey"], entry["reasonKey"], "{format}");
        }
    }
    Ok(())
}

#[test]
fn treemap_scope_total_equals_the_row_total_of_a_published_result() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&two_files(3))?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "treemap", 3))?;
    assert_eq!(answer["kind"], "model");
    assert_unwatched(&answer);
    let rows = rows_of(&harness, &answer)?;
    let listed: u64 = row_bytes(&rows)?.iter().sum();
    let scope_total = &answer["model"]["totals"]["scope"]["quantity"];
    assert_eq!(scope_total["value"], json!(listed.to_string()));
    assert_eq!(listed, 100);
    let shapes: Vec<_> = answer["model"]["shapes"]
        .as_array()
        .ok_or("no shapes")?
        .to_vec();
    assert_eq!(shapes.len(), 2);
    for shape in shapes {
        assert_eq!(shape["legendClassKey"], policy::LEGEND_CLASS_UNCLASSIFIED);
        assert_eq!(shape["displayFlags"], json!([]));
    }
    Ok(())
}

#[test]
fn bar_over_the_shape_limit_serves_the_other_group() -> TestResult {
    let harness = Harness::product(4)?;
    let files: Vec<_> = (0..8_u8)
        .map(|n| (b'a' + n, Some(10 * (u64::from(n) + 1))))
        .collect();
    harness.publish(&Publication {
        files: &files,
        ..two_files(3)
    })?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "bar", 3))?;
    assert_eq!(answer["kind"], "model", "{answer}");
    assert_unwatched(&answer);
    let groups: Vec<_> = answer["model"]["shapes"]
        .as_array()
        .ok_or("no shapes")?
        .iter()
        .filter(|shape| !shape["groupRef"].is_null())
        .collect();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["groupRef"], policy::GROUP_OTHER_REF);
    assert_eq!(groups[0]["displayFlags"], json!([policy::FLAG_OTHER_GROUP]));
    let rows = rows_of(&harness, &answer)?;
    let other = rows["rows"]
        .as_array()
        .ok_or("no rows")?
        .iter()
        .find(|row| !row["groupRef"].is_null())
        .ok_or("no group row")?;
    assert_eq!(other["displayName"], policy::GROUP_OTHER_NAME_KEY);
    assert_eq!(other["labelKey"], policy::GROUP_OTHER_NAME_KEY);
    assert_eq!(other["legendClassKey"], policy::LEGEND_CLASS_UNCLASSIFIED);
    let legend = answer["model"]["legend"].as_array().ok_or("no legend")?;
    assert_eq!(legend.len(), 1);
    assert_eq!(legend[0]["labelKey"], policy::LEGEND_LABEL_UNCLASSIFIED);
    Ok(())
}

#[test]
fn histogram_follows_the_unit_setting_with_symbol_only_interval_names() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&two_files(3))?;
    let view = harness.open_view()?;
    for (seq, (iec, first)) in (1..).zip([(false, "0 B\u{2013}1 kB"), (true, "0 B\u{2013}1 KiB")]) {
        harness.iec.store(iec, Ordering::SeqCst);
        let answer = harness.ok("viz.render", render_body(&view, seq, "histogram", 3))?;
        assert_eq!(answer["kind"], "model", "{answer}");
        assert_unwatched(&answer);
        let rows = rows_of(&harness, &answer)?;
        let names: Vec<_> = rows["rows"]
            .as_array()
            .ok_or("no rows")?
            .iter()
            .filter(|row| !row["groupRef"].is_null())
            .filter_map(|row| row["displayName"].as_str())
            .collect();
        assert_eq!(names.len(), 6, "{names:?}");
        assert!(names.contains(&first), "{names:?}");
    }
    Ok(())
}

#[test]
fn an_empty_folder_renders_an_empty_model_in_every_runtime_format() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&Publication {
        files: &[],
        ..two_files(3)
    })?;
    let view = harness.open_view()?;
    for (seq, format) in (1..).zip(["treemap", "bar", "histogram"]) {
        let answer = harness.ok("viz.render", render_body(&view, seq, format, 3))?;
        assert_eq!(answer["kind"], "model", "{format}: {answer}");
        assert_unwatched(&answer);
        assert_eq!(answer["model"]["shapes"], json!([]), "{format}");
        let rows = rows_of(&harness, &answer)?;
        assert!(
            rows["rows"]
                .as_array()
                .is_some_and(|rows| rows.iter().all(|r| r["itemRef"].is_null())),
            "{format}"
        );
        if format != "histogram" {
            assert_eq!(rows["totalCount"], 0, "{format}");
            assert_eq!(answer["model"]["totals"]["scope"]["quantity"]["value"], "0");
        }
    }
    Ok(())
}

#[test]
fn a_child_list_without_any_size_value_is_missing_item_sizes() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&Publication {
        files: &[(b'a', None), (b'b', None)],
        ..two_files(3)
    })?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "treemap", 3))?;
    assert_eq!(answer["kind"], "unavailable");
    assert_eq!(answer["reasonKey"], "viz.unavailable.no_item_sizes");
    Ok(())
}

#[test]
fn a_classification_criterion_without_a_rule_table_is_an_invalid_argument() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&two_files(3))?;
    let view = harness.open_view()?;
    for (seq, format) in (1..).zip(["treemap", "bar", "histogram"]) {
        let mut body = render_body(&view, seq, format, 3);
        body["classificationCriterion"] = json!("type");
        let error = harness
            .call("viz.render", body)
            .error
            .ok_or("a render with an unserved criterion succeeded")?;
        assert_eq!(error.reason_key, "viz.measure_unknown", "{format}");
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{format}");
    }
    Ok(())
}
