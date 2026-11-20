use super::*;
use du_core_contracts::result::ResultItemKind;

// #183: every shape and row of one model carries the kind of its pinned item, a shape
// and a row for the same item agree, and a shape with children is a folder.
#[test]
fn shapes_and_rows_carry_the_pinned_item_kind_and_agree() -> Result<(), String> {
    let fixture = Fixture::with_tree(
        [25, 75],
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
        true,
    )?;
    let view = fixture.open()?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let mut request = render_body(&view, 1);
    request["depthLimit"] = json!(2);
    let model = fixture.ok("viz.render", request)?;
    assert_eq!(model["kind"], "model");
    let pinned = fixture
        .sink
        .pin_current(&id(3))
        .map_err(|e| format!("{e:?}"))?;
    let expected = |item: &Value| -> Result<Value, String> {
        let item: ItemRef = serde_json::from_value(item.clone()).map_err(|e| e.to_string())?;
        let detail = pinned.detail(&item).map_err(|e| format!("{e:?}"))?;
        Ok(match detail.item.kind {
            ResultItemKind::Directory => json!("folder"),
            ResultItemKind::File => json!("file"),
            ResultItemKind::UntraversedLink => json!("other"),
            ResultItemKind::Unknown | ResultItemKind::UnknownStructure => Value::Null,
        })
    };
    let shapes = model["model"]["shapes"].as_array().ok_or("no shapes")?;
    let mut seen = Vec::new();
    for shape in shapes {
        if shape["itemRef"].is_null() {
            assert!(shape.get("itemKind").is_none(), "group shape {shape}");
            continue;
        }
        assert_eq!(shape["itemKind"], expected(&shape["itemRef"])?, "{shape}");
        seen.push(shape["itemKind"].clone());
        if shapes
            .iter()
            .any(|child| child["parentShapeRef"] == shape["shapeRef"])
        {
            assert_eq!(shape["itemKind"], "folder", "{shape}");
        }
    }
    assert!(seen.contains(&json!("folder")) && seen.contains(&json!("file")));
    let rows = fixture.ok(
        "viz.rows",
        json!({"modelRef": model["model"]["modelRef"], "resultRevision": id(3), "start": 0, "count": 100}),
    )?;
    let rows = rows["rows"].as_array().ok_or("no rows")?;
    assert!(!rows.is_empty());
    for row in rows {
        if row["itemRef"].is_null() {
            assert!(row.get("itemKind").is_none(), "group row {row}");
            continue;
        }
        assert_eq!(row["itemKind"], expected(&row["itemRef"])?, "{row}");
        for shape in shapes
            .iter()
            .filter(|shape| shape["itemRef"] == row["itemRef"])
        {
            assert_eq!(shape["itemKind"], row["itemKind"], "{row}");
        }
    }
    Ok(())
}
