//! Lower-bound siblings of a treemap served through the product registration.
//!
//! A link that was not followed, or an entry that could not be read, makes the F03
//! value above it a lower bound. The scope is a model whose shapes carry the lower-bound
//! mark and whose totals carry the state, not a table-only fallback.
use du_analysis_index::{IngestSink, Observation};
use du_core_contracts::{
    PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use serde_json::{Value, json};

use super::support::{Harness, id, render_body};
use super::totals::assert_within_scope;
use super::{TestResult, rows_of};
use crate::viz_production::policy;

enum Entry {
    Dir(&'static str),
    File(&'static str, u64),
    Link(&'static str),
    Unread(&'static str),
}

fn publish(
    sink: &IngestSink,
    entries: &[Entry],
    completeness: ResultCompleteness,
    gaps: &[&str],
) -> Result<(), String> {
    let debug = |error: &dyn std::fmt::Debug| format!("{error:?}");
    let lineage = sink
        .bind_current_analysis("scope-lower-bound", id(1))
        .map_err(|error| debug(&error))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut scan = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|error| debug(&error))?;
    scan.add_directory(vec![]).map_err(|error| debug(&error))?;
    let path = |text: &str| -> Vec<Vec<u8>> {
        text.split('/')
            .map(|part| part.as_bytes().to_vec())
            .collect()
    };
    for entry in entries {
        match entry {
            Entry::Dir(at) => scan.add_directory(path(at)),
            Entry::File(at, bytes) => scan.add_file(Observation {
                path: path(at),
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: (*bytes).into(),
                    },
                }),
            }),
            Entry::Link(at) => scan.add_untraversed_link(path(at)),
            Entry::Unread(at) => scan.add(Observation {
                path: path(at),
                size: None,
            }),
        }
        .map_err(|error| debug(&error))?;
    }
    for gap in gaps {
        scan.record_gap_at(0, path(gap), "scan.path_denied".into(), None)
            .map_err(|error| debug(&error))?;
    }
    scan.seal(id(3), completeness)
        .map_err(|error| debug(&error))?;
    Ok(())
}

fn render_in(harness: &Harness, view: &Value, seq: u32, item: u64) -> Result<Value, String> {
    let mut body = render_body(view, seq, "treemap", 3);
    let scope = json!({"resultVersion": id(3), "itemNo": item});
    body["aggregateScopeRef"] = scope.clone();
    body["navigationScopeRef"] = scope;
    let answer = harness.ok("viz.render", body)?;
    assert_eq!(answer["kind"], "model", "{answer}");
    Ok(answer)
}

fn total(model: &Value, name: &str) -> Result<(String, u64), String> {
    let quantity = &model["totals"][name]["quantity"];
    let state = quantity["state"].as_str().ok_or("no state")?.to_owned();
    let value = quantity["value"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("no {name} value in {}", model["totals"]))?;
    Ok((state, value))
}

fn count(model: &Value, name: &str) -> Result<u64, String> {
    model["summary"][name]["value"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("no {name} count in {}", model["summary"]))
}

fn item_no(rows: &Value, name: &str) -> Result<u64, String> {
    rows["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["displayName"] == name))
        .and_then(|row| row["itemRef"]["itemNo"].as_u64())
        .ok_or_else(|| format!("no row named {name}"))
}

fn shape_flags(model: &Value, item: u64) -> Result<&Value, String> {
    model["shapes"]
        .as_array()
        .and_then(|shapes| {
            shapes
                .iter()
                .find(|shape| shape["itemRef"]["itemNo"] == item)
        })
        .map(|shape| &shape["displayFlags"])
        .ok_or_else(|| format!("no shape for item {item}"))
}

#[test]
fn a_scope_with_an_untraversed_link_is_a_model_with_a_lower_bound_total_and_mark() -> TestResult {
    let harness = Harness::product(2000)?;
    publish(
        &harness.sink,
        &[
            Entry::Dir("a"),
            Entry::Dir("x"),
            Entry::File("a/fa", 100),
            Entry::File("x/fx", 20),
            Entry::Link("x/lx"),
            Entry::File("top", 50),
        ],
        ResultCompleteness::Complete,
        &[],
    )?;
    let view = harness.open_view()?;
    let answer = render_in(&harness, &view, 1, 0)?;
    let model = &answer["model"];
    assert_eq!(model["shapes"].as_array().map(Vec::len), Some(3));
    // The scope and what is drawn are lower bounds, and they add up as they are.
    assert_eq!(total(model, "scope")?, ("lower_bound".into(), 170));
    assert_eq!(total(model, "drawn")?, ("lower_bound".into(), 170));
    assert_eq!(total(model, "grouped")?, ("exact".into(), 0));
    assert_eq!(total(model, "filteredOut")?, ("exact".into(), 0));
    assert_within_scope(model)?;
    assert_eq!(count(model, "unknownItems")?, 0);
    assert_eq!(model["summary"]["reasonKeys"], json!([]));
    let rows = rows_of(&harness, &answer)?;
    let x = item_no(&rows, "x")?;
    // Only the folder above the link carries the mark, on its shape and on its row.
    for name in ["a", "top"] {
        assert_eq!(shape_flags(model, item_no(&rows, name)?)?, &json!([]));
    }
    assert_eq!(shape_flags(model, x)?, &json!([policy::FLAG_LOWER_BOUND]));
    let row = rows["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["displayName"] == "x"))
        .ok_or("no row for x")?;
    assert_eq!(row["size"]["state"], "lower_bound");
    assert_eq!(row["displayFlags"], json!([policy::FLAG_LOWER_BOUND]));
    // A lower-bound parent is no denominator, so no row has a ratio.
    assert!(
        rows["rows"]
            .as_array()
            .ok_or("no rows")?
            .iter()
            .all(|row| row["ratio"].is_null())
    );

    // Inside the folder that holds the link the file is drawn, exact, and the link is
    // counted without an area; the scope stays a lower bound.
    let inner = render_in(&harness, &view, 2, x)?;
    let model = &inner["model"];
    assert_eq!(model["shapes"].as_array().map(Vec::len), Some(1));
    assert_eq!(count(model, "unknownItems")?, 1);
    assert_eq!(total(model, "scope")?, ("lower_bound".into(), 20));
    assert_eq!(total(model, "drawn")?, ("exact".into(), 20));
    assert_within_scope(model)
}

#[test]
fn a_scope_with_an_unread_entry_is_a_model_with_the_gap_reason_and_a_lower_bound_total()
-> TestResult {
    let harness = Harness::product(2000)?;
    publish(
        &harness.sink,
        &[
            Entry::Dir("a"),
            Entry::File("a/fa", 100),
            Entry::File("top", 50),
            Entry::Unread("locked"),
        ],
        ResultCompleteness::Missing,
        &["locked"],
    )?;
    let view = harness.open_view()?;
    let answer = render_in(&harness, &view, 1, 0)?;
    let model = &answer["model"];
    assert_eq!(model["shapes"].as_array().map(Vec::len), Some(2));
    // The reason is the range's own key; the unread entry has no area and is counted.
    assert_eq!(model["summary"]["reasonKeys"], json!(["scan.path_denied"]));
    assert_eq!(count(model, "unknownItems")?, 1);
    // The unread entry is directly under the scope, so the scope is a lower bound.
    assert_eq!(total(model, "scope")?, ("lower_bound".into(), 150));
    assert_within_scope(model)?;
    // Whether a folder that was read completely is a lower bound depends on how F03
    // treats a result with unread ranges: the whole result today, only the folders above
    // an unread range once FR5.HD1 is applied per folder. This holds for both: the mark of
    // each shape and the state of the drawn total follow the rows they come from.
    let rows = rows_of(&harness, &answer)?;
    let mut any_lower = false;
    for shape in model["shapes"].as_array().ok_or("no shapes")? {
        let row = rows["rows"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["itemRef"] == shape["itemRef"]))
            .ok_or("no row for a shape")?;
        let lower = row["size"]["state"] == "lower_bound";
        any_lower |= lower;
        let expected = if lower {
            json!([policy::FLAG_LOWER_BOUND])
        } else {
            json!([])
        };
        assert_eq!(shape["displayFlags"], expected);
    }
    let (state, drawn) = total(model, "drawn")?;
    assert_eq!(state == "lower_bound", any_lower);
    assert_eq!(drawn, 150);
    Ok(())
}
