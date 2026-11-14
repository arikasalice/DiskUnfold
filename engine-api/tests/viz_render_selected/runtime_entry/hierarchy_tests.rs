use super::*;

#[test]
fn depth_two_uses_pinned_descendants_without_changing_root_rows_or_totals() -> Result<(), String> {
    let fixture = Fixture::with_tree(
        [25, 75],
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
        true,
    )?;
    let view = fixture.open()?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let first = fixture.ok("viz.render", render_body(&view, 1))?;
    assert_eq!(first["kind"], "model");
    assert_eq!(first["model"]["dataSource"], "real");
    let mut request = render_body(&view, 2);
    request["depthLimit"] = json!(2);
    let second = fixture.ok("viz.render", request)?;
    assert_eq!(second["kind"], "model");
    assert_eq!(first["resultRevision"], second["resultRevision"]);
    assert_eq!(first["model"]["totals"], second["model"]["totals"]);
    assert_eq!(first["model"]["legend"], second["model"]["legend"]);
    let rows = |model: &Value| {
        fixture.ok("viz.rows", json!({"modelRef": model["model"]["modelRef"], "resultRevision": id(3), "start": 0, "count": 100}))
    };
    assert_eq!(rows(&first)?, rows(&second)?);
    let before = first["model"]["shapes"]
        .as_array()
        .ok_or("no first shapes")?;
    let after = second["model"]["shapes"]
        .as_array()
        .ok_or("no second shapes")?;
    assert_eq!(&after[..before.len()], before);
    assert!(before.len() >= 4);
    assert!(after.len() > before.len());
    assert_branches_expanded(before, after);
    let pinned = fixture
        .sink
        .pin_current(&id(3))
        .map_err(|e| format!("{e:?}"))?;
    for (offset, nested) in after[before.len()..].iter().enumerate() {
        assert_eq!(nested["visibleDepth"], 2);
        assert!(
            before
                .iter()
                .any(|parent| parent["shapeRef"] == nested["parentShapeRef"]
                    && parent["itemRef"] == nested["rootBranchRef"])
        );
        assert_eq!(nested["itemRef"]["resultVersion"], json!(id(3)));
        let item: ItemRef =
            serde_json::from_value(nested["itemRef"].clone()).map_err(|e| e.to_string())?;
        assert_eq!(
            nested["size"],
            json!(
                pinned
                    .item_size(item.item_no, SizeBasis::Logical)
                    .ok_or("missing size")?
            )
        );
        let located = fixture.ok(
            "viz.locate",
            json!({"modelRef":second["model"]["modelRef"], "itemRef":nested["itemRef"]}),
        )?;
        assert_eq!(located["location"], "shape");
        assert_eq!(located["shapeIndex"], json!(before.len() + offset));
    }
    let mut limited = render_body(&view, 3);
    limited["depthLimit"] = json!(2);
    limited["viewport"]["width"] = json!(4);
    limited["viewport"]["height"] = json!(4);
    limited["minAreaPx2"] = json!(4);
    let limited = fixture.ok("viz.render", limited)?;
    assert_eq!(limited["kind"], "model");
    assert!(
        limited["model"]["shapes"]
            .as_array()
            .ok_or("missing shapes")?
            .len()
            <= 4
    );
    assert_eq!(limited["model"]["totals"], first["model"]["totals"]);
    Ok(())
}

fn assert_branches_expanded(before: &[Value], after: &[Value]) {
    for parent in before {
        assert!(
            after[before.len()..]
                .iter()
                .any(|child| child["parentShapeRef"] == parent["shapeRef"]),
            "each populated branch is expanded"
        );
    }
}

#[test]
fn local_expansion_keeps_the_parent_and_only_opens_the_selected_branch() -> Result<(), String> {
    let fixture = Fixture::with_tree(
        [25, 75],
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
        true,
    )?;
    let view = fixture.open()?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let initial = fixture.ok("viz.render", render_body(&view, 1))?;
    let before = initial["model"]["shapes"].as_array().ok_or("no shapes")?;
    let selected = before.first().ok_or("no branch to expand")?;
    let mut request = render_body(&view, 2);
    request["expandedDepthByItem"] = json!([{"itemRef": selected["itemRef"], "extraDepth": 1}]);
    let expanded = fixture.ok("viz.render", request)?;
    let after = expanded["model"]["shapes"]
        .as_array()
        .ok_or("no expanded shapes")?;
    assert!(after.len() > before.len());
    assert_eq!(&after[..before.len()], before);
    assert!(
        after[before.len()..]
            .iter()
            .all(|shape| shape["visibleDepth"] == 2
                && shape["parentShapeRef"] == selected["shapeRef"]
                && shape["rootBranchRef"] == selected["itemRef"])
    );
    assert_eq!(initial["resultRevision"], expanded["resultRevision"]);
    assert_eq!(initial["model"]["totals"], expanded["model"]["totals"]);
    assert_eq!(initial["model"]["legend"], expanded["model"]["legend"]);
    let sibling = before.get(1).ok_or("no sibling to validate")?;
    let mut equivalent = render_body(&view, 3);
    equivalent["expandedDepthByItem"] = json!([
        {"itemRef": sibling["itemRef"], "extraDepth": 0},
        {"itemRef": selected["itemRef"], "extraDepth": 1}
    ]);
    let equivalent = fixture.ok("viz.render", equivalent)?;
    assert_eq!(
        expanded["model"]["modelRef"],
        equivalent["model"]["modelRef"]
    );
    Ok(())
}

#[test]
fn expansion_limits_have_policy_errors_and_invalid_entries_are_rejected() -> Result<(), String> {
    let fixture = Fixture::new([25, 75])?;
    let view = fixture.open()?;
    let entry = |number, depth| {
        json!({
            "itemRef":{"resultVersion":id(3),"itemNo":number},"extraDepth":depth
        })
    };
    for (entries, reason) in [
        (json!([entry(0, 9)]), "viz.viewport_invalid"),
        (
            json!((0..65).map(|number| entry(number, 0)).collect::<Vec<_>>()),
            "viz.viewport_invalid",
        ),
        (json!([entry(0, 0), entry(0, 0)]), "command.body_invalid"),
        (json!([entry(1000, 0)]), "command.body_invalid"),
    ] {
        let mut request = render_body(&view, 1);
        request["expandedDepthByItem"] = entries;
        assert_eq!(
            fixture
                .call("viz.render", request)
                .error
                .ok_or("invalid expansion accepted")?
                .reason_key,
            reason
        );
    }
    Ok(())
}
