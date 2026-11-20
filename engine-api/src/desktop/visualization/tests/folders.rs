//! Ranked bars over a folder whose children are folders (#242).
//!
//! The F03 set total takes file rows only, so the drawn and Other totals of folder
//! bars come from the exact partition of the scope's children instead.
use du_analysis_index::Observation;
use du_core_contracts::{
    PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};

use super::TestResult;
use super::support::{Harness, id, render_body};

/// Publishes revision 3: three folders and a file under the root, like the ui-smoke
/// treemap fixture.
fn publish_folders(harness: &Harness) -> Result<(), String> {
    let debug = |error: &dyn std::fmt::Debug| format!("{error:?}");
    let lineage = harness
        .sink
        .bind_current_analysis("scope-folders", id(1))
        .map_err(|error| debug(&error))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut scan = harness
        .sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|error| debug(&error))?;
    scan.add_directory(vec![]).map_err(|error| debug(&error))?;
    let file = |path: Vec<Vec<u8>>, bytes: u64| Observation {
        path,
        size: Some(SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount: SizeAmount::Exact {
                bytes: bytes.into(),
            },
        }),
    };
    for (folder, files) in [
        (b"Videos".as_slice(), vec![(b"clip".as_slice(), 128_u64)]),
        (b"Photos", vec![(b"album-a", 64), (b"album-b", 48)]),
        (b"Tiny", vec![(b"tiny", 1)]),
    ] {
        scan.add_directory(vec![folder.to_vec()])
            .map_err(|error| debug(&error))?;
        for (name, bytes) in files {
            scan.add_file(file(vec![folder.to_vec(), name.to_vec()], bytes))
                .map_err(|error| debug(&error))?;
        }
    }
    scan.add_file(file(vec![b"notes".to_vec()], 16))
        .map_err(|error| debug(&error))?;
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|error| debug(&error))?;
    Ok(())
}

#[test]
fn a_bar_chart_of_folders_draws_ranked_bars() -> TestResult {
    let harness = Harness::product(2000)?;
    publish_folders(&harness)?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "bar", 3))?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let shapes = answer["model"]["shapes"].as_array().ok_or("no shapes")?;
    assert_eq!(shapes.len(), 4, "{answer}");
    Ok(())
}

#[test]
fn a_bar_chart_of_folders_over_the_shape_limit_draws_the_other_group() -> TestResult {
    let harness = Harness::product(3)?;
    publish_folders(&harness)?;
    let view = harness.open_view()?;
    let answer = harness.ok("viz.render", render_body(&view, 1, "bar", 3))?;
    assert_eq!(answer["kind"], "model", "{answer}");
    let groups = answer["model"]["shapes"]
        .as_array()
        .ok_or("no shapes")?
        .iter()
        .filter(|shape| !shape["groupRef"].is_null())
        .count();
    assert_eq!(groups, 1, "{answer}");
    Ok(())
}
