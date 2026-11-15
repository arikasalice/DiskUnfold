use super::*;
use du_core_contracts::visualization::response::VizRenderResponse;

// Folder big holds a hundred files of distinct sizes 1..=100 bytes, which also keeps
// the fixture independent of sibling tie ordering; folder other holds one large file.
fn publish_many(sink: &IngestSink) -> Result<(), String> {
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|error| format!("{error:?}"))?;
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
        .map_err(|error| format!("{error:?}"))?;
    for dir in [vec![], vec![b"big".to_vec()], vec![b"other".to_vec()]] {
        scan.add_directory_at(0, dir)
            .map_err(|error| format!("{error:?}"))?;
    }
    let mut files: Vec<(Vec<Vec<u8>>, u64)> = (0..100)
        .map(|i| {
            (
                vec![b"big".to_vec(), format!("f{i:03}").into_bytes()],
                i + 1,
            )
        })
        .collect();
    files.push((vec![b"other".to_vec(), b"file".to_vec()], 10_000));
    for (path, bytes) in files {
        scan.add_file_at(
            0,
            Observation {
                path,
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: bytes.into(),
                    },
                }),
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

#[test]
fn an_expanded_folder_groups_its_small_children_and_opens_them_by_position() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_many(&sink)?;
    let fixture = Fixture::from_sink(
        sink,
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
    )?;
    let view = fixture.open()?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let mut request = render_body(&view, 1);
    request["depthLimit"] = json!(2);
    request["viewport"]["width"] = json!(40);
    request["viewport"]["height"] = json!(40);
    request["minAreaPx2"] = json!(16);
    let rendered = fixture.ok("viz.render", request)?;
    let parsed: VizRenderResponse =
        serde_json::from_value(rendered.clone()).map_err(|e| e.to_string())?;
    parsed.validate()?;
    let model = &rendered["model"];
    let shapes = model["shapes"].as_array().ok_or("no shapes")?;
    assert!(shapes.len() <= 100);
    let group = shapes
        .iter()
        .find(|shape| shape["groupRef"].is_string())
        .ok_or_else(|| format!("no group shape: {}", model["summary"]))?;
    let parent = shapes
        .iter()
        .find(|shape| shape["shapeRef"] == group["parentShapeRef"])
        .ok_or("the group has no parent shape")?;
    assert_eq!(group["visibleDepth"], 2);
    assert_eq!(group["rootBranchRef"], parent["itemRef"]);
    let count = |value: &Value| -> Result<u64, String> {
        value["value"]
            .as_str()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| format!("not a count: {value}"))
    };
    // The minimum area is the whole window over the shape limit, not a share of the
    // parent, so every file of the folder is below it and all of them merge.
    assert_eq!(count(&model["summary"]["groupedItems"])?, 100);
    // Two folders, the group of the first, and the single file of the second.
    assert_eq!(shapes.len(), 4);
    // A group inside a drawn folder is part of that folder's bytes: only the root
    // group counts in the grouped total.
    assert_eq!(model["totals"]["grouped"]["quantity"]["value"], "0");
    assert_eq!(model["totals"]["drawn"]["quantity"]["value"], "15050");
    let members = fixture.ok(
        "viz.group_rows",
        json!({"modelRef": model["modelRef"], "groupRef": group["groupRef"], "start": 0, "count": 1000}),
    )?;
    assert_eq!(members["totalCount"], 100);
    assert_eq!(members["groupTotal"], group["size"]);
    // Members are item rows: only a group row the engine made may carry a label key.
    assert!(
        members["rows"]
            .as_array()
            .ok_or("no member rows")?
            .iter()
            .all(|row| row.get("labelKey").is_none() && row["groupRef"].is_null())
    );
    // The page contract is the existing one: 1 and 1000 are read, 0 and 1001 refused.
    for (count, accepted) in [(1, true), (1000, true), (0, false), (1001, false)] {
        let answer = fixture.call(
            "viz.group_rows",
            json!({"modelRef": model["modelRef"], "groupRef": group["groupRef"], "start": 0, "count": count}),
        );
        assert_eq!(answer.error.is_none(), accepted, "count {count}");
    }
    let located = fixture.ok(
        "viz.locate",
        json!({"modelRef": model["modelRef"], "itemRef": members["rows"][99]["itemRef"]}),
    )?;
    assert_eq!(located["location"], "group");
    assert_eq!(located["groupRef"], group["groupRef"]);
    // Like every shape inside an expanded folder, the nested group has no table row.
    let rows = fixture.ok(
        "viz.rows",
        json!({"modelRef": model["modelRef"], "resultRevision": id(3), "start": 0, "count": 100}),
    )?;
    assert_eq!(rows["totalCount"], 2);
    assert!(
        rows["rows"]
            .as_array()
            .ok_or("no rows")?
            .iter()
            .all(|row| row["groupRef"].is_null())
    );
    Ok(())
}
