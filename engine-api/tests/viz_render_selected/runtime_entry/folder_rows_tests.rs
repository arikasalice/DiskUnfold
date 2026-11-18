//! The rows of an expanded folder, read by the shape reference of the folder through
//! the registered `viz.rows` command, and the member count of the groups it shows.
use super::nested_group_tests::publish_many;
use super::*;
use du_core_contracts::{ErrorCode, visualization::response::VizRenderResponse};

fn opened() -> Result<(Fixture, Value), String> {
    let sink = IngestSink::default();
    publish_many(&sink)?;
    let fixture = Fixture::from_sink(
        sink,
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
    )?;
    let view = fixture.open()?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    Ok((fixture, view))
}

// Folder big holds a hundred small files that merge into one group when it is opened;
// folder other holds one large file, which is drawn as a shape of its own.
fn render_depth(fixture: &Fixture, view: &Value, seq: u32, depth: u32) -> Result<Value, String> {
    let mut request = render_body(view, seq);
    request["depthLimit"] = json!(depth);
    request["viewport"]["width"] = json!(40);
    request["viewport"]["height"] = json!(40);
    let rendered = fixture.ok("viz.render", request)?;
    let parsed: VizRenderResponse =
        serde_json::from_value(rendered.clone()).map_err(|e| e.to_string())?;
    parsed.validate()?;
    Ok(rendered["model"].clone())
}

fn rows_of(fixture: &Fixture, model: &Value, parent: Option<&str>) -> ResultEnvelope {
    let mut body =
        json!({"modelRef": model["modelRef"], "resultRevision": id(3), "start": 0, "count": 1000});
    if let Some(parent) = parent {
        body["parentShapeRef"] = json!(parent);
    }
    fixture.call("viz.rows", body)
}

fn shapes(model: &Value) -> Result<&Vec<Value>, String> {
    model["shapes"].as_array().ok_or_else(|| "no shapes".into())
}

/// The shape reference of the root folder of this name, found through the root table.
fn folder_shape(fixture: &Fixture, model: &Value, name: &str) -> Result<String, String> {
    let root = rows_of(fixture, model, None).body;
    let row = root["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["displayName"] == name))
        .ok_or_else(|| format!("no root row {name}"))?;
    shapes(model)?
        .iter()
        .find(|shape| shape["itemRef"] == row["itemRef"])
        .and_then(|shape| shape["shapeRef"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("no shape for {name}"))
}

/// The shape drawn directly inside the folder with this shape reference.
fn child_shape<'a>(model: &'a Value, parent: &str) -> Result<&'a Value, String> {
    shapes(model)?
        .iter()
        .find(|shape| shape["parentShapeRef"] == parent)
        .ok_or_else(|| format!("nothing is drawn inside {parent}"))
}

fn items(value: &str) -> Value {
    json!({"unit": "items", "state": "exact", "value": value})
}

/// The page contract is the one of the root table: the same limits, and a start past the
/// end of this folder's one-row table is refused, while a start at its end is an empty page.
fn check_page_contract(fixture: &Fixture, model: &Value, folder: &str) -> Result<(), String> {
    let page = |start: u32, count: u32| {
        fixture.call(
            "viz.rows",
            json!({"modelRef": model["modelRef"], "resultRevision": id(3), "start": start, "count": count, "parentShapeRef": folder}),
        )
    };
    for (count, accepted) in [(1, true), (1000, true), (0, false), (1001, false)] {
        assert_eq!(page(0, count).error.is_none(), accepted, "count {count}");
    }
    let past = page(2, 1).error.ok_or("a start past the table")?;
    assert_eq!(past.reason_key, "command.body_invalid");
    let end = page(1, 1);
    assert_eq!(end.body["rows"], json!([]));
    assert_eq!(end.body["totalCount"], 1);
    Ok(())
}
#[test]
fn the_rows_of_an_expanded_folder_name_its_shapes_by_item_and_group_reference() -> Result<(), String>
{
    let (fixture, view) = opened()?;
    let model = render_depth(&fixture, &view, 1, 2)?;
    let (big, other) = (
        folder_shape(&fixture, &model, "big")?,
        folder_shape(&fixture, &model, "other")?,
    );

    // The root table is what it was: the two folders, and no member count on an item row.
    let root = rows_of(&fixture, &model, None).body;
    assert_eq!(root["totalCount"], 2);
    assert!(
        root["rows"]
            .as_array()
            .ok_or("no rows")?
            .iter()
            .all(|row| row.get("memberCount").is_none())
    );

    // A folder that drew a shape: its one row is that shape's item, by item reference.
    let drawn = child_shape(&model, &other)?;
    let table = rows_of(&fixture, &model, Some(&other)).body;
    assert_eq!(table["totalCount"], 1);
    let row = &table["rows"][0];
    assert_eq!(row["itemRef"], drawn["itemRef"]);
    assert_eq!(row["groupRef"], Value::Null);
    assert_eq!(row["displayName"], "file");
    assert_eq!(row["size"], drawn["size"]);
    assert!(row.get("memberCount").is_none() && row.get("labelKey").is_none());

    // A folder that merged its small children: its one row is the group, by group
    // reference, with the count of its members on the row and on the shape alike.
    let group = child_shape(&model, &big)?;
    let table = rows_of(&fixture, &model, Some(&big)).body;
    assert_eq!(table["totalCount"], 1);
    let row = &table["rows"][0];
    assert_eq!(row["itemRef"], Value::Null);
    assert_eq!(row["groupRef"], group["groupRef"]);
    assert_eq!(row["size"], group["size"]);
    assert_eq!(row["memberCount"], items("100"));
    assert_eq!(group["memberCount"], items("100"));
    let members = fixture.ok(
        "viz.group_rows",
        json!({"modelRef": model["modelRef"], "groupRef": group["groupRef"], "start": 0, "count": 1}),
    )?;
    assert_eq!(members["totalCount"], 100);
    assert_eq!(model["summary"]["groupedItems"], items("100"));

    check_page_contract(&fixture, &model, &big)
}

#[test]
fn a_reference_the_model_lacks_is_not_found_and_a_shape_that_is_no_expanded_folder_is_invalid()
-> Result<(), String> {
    let (fixture, view) = opened()?;
    let model = render_depth(&fixture, &view, 1, 2)?;
    let (big, other) = (
        folder_shape(&fixture, &model, "big")?,
        folder_shape(&fixture, &model, "other")?,
    );
    let file = child_shape(&model, &other)?["shapeRef"]
        .as_str()
        .ok_or("no file shape reference")?
        .to_owned();
    let group = child_shape(&model, &big)?["shapeRef"]
        .as_str()
        .ok_or("no group shape reference")?
        .to_owned();
    let refused = |reference: &str, model: &Value| -> Result<(ErrorCode, String), String> {
        let error = rows_of(&fixture, model, Some(reference))
            .error
            .ok_or_else(|| format!("{reference} was read"))?;
        Ok((error.code, error.reason_key))
    };
    let invalid = (
        ErrorCode::InvalidArgument,
        "command.body_invalid".to_owned(),
    );
    let missing = (ErrorCode::NotFound, "ref.unavailable".to_owned());

    // A shape of this model that is a leaf or a group has no children to list.
    assert_eq!(refused(&file, &model)?, invalid);
    assert_eq!(refused(&group, &model)?, invalid);
    // A reference no shape of the model has.
    assert_eq!(refused("no-such-shape", &model)?, missing);

    // A second model of the same scope that opens no folder: both folders are shapes
    // of it, but neither was expanded, so they are refused like a leaf. The references of
    // the shapes that exist only inside the first model are not found in the second.
    let shallow = render_depth(&fixture, &view, 2, 1)?;
    assert_ne!(shallow["modelRef"], model["modelRef"]);
    assert_eq!(refused(&big, &shallow)?, invalid);
    assert_eq!(refused(&other, &shallow)?, invalid);
    assert_eq!(refused(&file, &shallow)?, missing);
    assert_eq!(refused(&group, &shallow)?, missing);
    // The first model still answers for its own shapes.
    assert!(rows_of(&fixture, &model, Some(&big)).error.is_none());
    Ok(())
}
