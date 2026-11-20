//! The "type" color criterion through the desktop assembly (F06 FR6).
use serde_json::{Value, json};

use super::support::{Harness, Publication, render_body};
use super::{TestResult, rows_of, two_files};
use crate::viz_production::policy;

fn mixed() -> Publication<'static> {
    Publication {
        files: &[],
        named: &[
            (&[b"report.pdf"], Some(100)),
            (&[b"song.mp3"], Some(50)),
            (&[b"setup.iso"], Some(400)),
            (&[b"Makefile"], Some(10)),
            (&[b"main.rs"], Some(5)),
            (&[b"photos", b"a.jpg"], Some(30)),
        ],
        ..two_files(3)
    }
}

fn classes(rows: &Value) -> Result<Vec<(String, String)>, String> {
    rows["rows"]
        .as_array()
        .ok_or("no rows")?
        .iter()
        .map(|row| {
            Ok((
                row["displayName"].as_str().ok_or("no name")?.to_owned(),
                row["legendClassKey"].as_str().ok_or("no class")?.to_owned(),
            ))
        })
        .collect()
}

#[test]
fn the_type_criterion_classes_files_by_type_and_leaves_folders_out_of_the_legend() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&mixed())?;
    let view = harness.open_view()?;
    let mut body = render_body(&view, 1, "treemap", 3);
    body["colorCriterion"] = json!(policy::COLOR_CRITERION_TYPE);
    let answer = harness.ok("viz.render", body)?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let mut found = classes(&rows_of(&harness, &answer)?)?;
    found.sort();
    assert_eq!(
        found,
        [
            ("Makefile", "type.no_extension"),
            ("main.rs", "type.unclassified"),
            ("photos", "type.folder"),
            ("report.pdf", "type.document"),
            ("setup.iso", "type.installer_image"),
            ("song.mp3", "type.audio"),
        ]
        .map(|(name, key)| (name.to_owned(), key.to_owned()))
    );
    let model = &answer["model"];
    let legend: Vec<_> = model["legend"]
        .as_array()
        .ok_or("no legend")?
        .iter()
        .map(|entry| entry["key"].clone())
        .collect();
    // The fixed order of the type legend, the folder left out.
    assert_eq!(
        legend,
        [
            "type.document",
            "type.audio",
            "type.installer_image",
            "type.unclassified",
            "type.no_extension",
        ]
        .map(|key| json!(key))
    );
    assert_eq!(model["tableRevisions"]["fileType"], 1);
    assert_eq!(model["representation"]["colorCriterion"], "type");
    Ok(())
}

#[test]
fn without_the_type_criterion_every_item_shares_one_class_and_no_table_revision() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&mixed())?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "treemap", 3))?;
    let found = classes(&rows_of(&harness, &answer)?)?;
    assert!(
        found
            .iter()
            .all(|(_, key)| key == policy::LEGEND_CLASS_UNCLASSIFIED),
        "{found:?}"
    );
    assert!(answer["model"]["tableRevisions"].get("fileType").is_none());
    Ok(())
}
