//! Which result revision a request is served from: a named one, a lineage's current one, or the
//! only current result.
use serde_json::json;

use super::support::{Harness, Publication, id, render_body};
use super::{TestResult, scope, two_files};

#[test]
fn an_unnamed_availability_request_for_a_revision_that_is_not_current_is_stale() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&two_files(3))?;
    // Without a lineage only the named revision is pinned; one that is not the
    // current scan result is refused as stale, as viz.render does.
    let reason = harness.reason(
        "viz.availability",
        json!({"resultRevision": id(7), "aggregateScopeRef": scope(7)}),
    )?;
    assert_eq!(reason, "result.version_stale");
    Ok(())
}

#[test]
fn a_named_revision_that_is_no_longer_current_is_stale() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&two_files(3))?;
    harness.publish(&two_files(5))?;
    let view = harness.open_view()?;
    let reason = harness.reason("viz.render", render_body(&view, 1, "treemap", 3))?;
    assert_eq!(reason, "result.version_stale");
    let current = harness.ok("viz.render", render_body(&view, 2, "treemap", 5))?;
    assert_eq!(current["resultRevision"], json!(id(5)));
    Ok(())
}

#[test]
fn a_publication_in_another_lineage_does_not_disturb_a_named_request() -> TestResult {
    let harness = Harness::product(2000)?;
    let first = serde_json::to_value(harness.publish(&two_files(3))?).map_err(|e| e.to_string())?;
    harness.publish(&Publication {
        fingerprint: "scope-b",
        lineage_candidate: 9,
        scope: 5,
        revision: 6,
        files: &[(b'x', Some(1))],
        named: &[],
    })?;
    let view = harness.open_view()?;
    let named = harness.ok("viz.render", render_body(&view, 1, "treemap", 3))?;
    assert_eq!(named["resultRevision"], json!(id(3)));
    // Naming only the lineage resolves that lineage's own current revision.
    let mut by_lineage = render_body(&view, 2, "treemap", 3);
    by_lineage["sourceRef"] = json!({"kind": "scan", "ref": first});
    by_lineage
        .as_object_mut()
        .ok_or("not an object")?
        .remove("resultRevision");
    let resolved = harness.ok("viz.render", by_lineage)?;
    assert_eq!(resolved["resultRevision"], json!(id(3)));
    Ok(())
}

#[test]
fn naming_neither_lineage_nor_revision_serves_the_only_current_result() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&two_files(3))?;
    let view = harness.open_view()?;
    let mut body = render_body(&view, 1, "treemap", 3);
    body.as_object_mut()
        .ok_or("not an object")?
        .remove("resultRevision");
    let answer = harness.ok("viz.render", body)?;
    assert_eq!(answer["resultRevision"], json!(id(3)));
    Ok(())
}
