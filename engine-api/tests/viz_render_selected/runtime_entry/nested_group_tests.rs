use super::*;
use du_core_contracts::visualization::response::VizRenderResponse;

// Folder big holds a hundred files of distinct sizes 1..=100 bytes, which also keeps
// the fixture independent of sibling tie ordering; folder other holds one large file.
pub(super) fn publish_many(sink: &IngestSink) -> Result<(), String> {
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

fn publish_root_and_nested_groups(sink: &IngestSink) -> Result<(), String> {
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
    for directory in [vec![], vec![b"big".to_vec()], vec![b"other".to_vec()]] {
        scan.add_directory_at(0, directory)
            .map_err(|error| format!("{error:?}"))?;
    }
    let exact = |bytes: u64| SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    };
    let mut files: Vec<(Vec<Vec<u8>>, u64)> = (0..40)
        .map(|i| (vec![format!("root{i:03}").into_bytes()], i + 1))
        .collect();
    files.extend((0..100).map(|i| {
        (
            vec![b"big".to_vec(), format!("child{i:03}").into_bytes()],
            i + 1,
        )
    }));
    files.push((vec![b"other".to_vec(), b"file".to_vec()], 10_000));
    for (path, bytes) in files {
        scan.add_file_at(
            0,
            Observation {
                path,
                size: Some(exact(bytes)),
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

#[test]
fn grouped_item_summary_matches_root_and_nested_groups_without_member_overlap() -> Result<(), String>
{
    let sink = IngestSink::default();
    publish_root_and_nested_groups(&sink)?;
    let fixture = Fixture::from_sink(
        sink,
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
    )?;
    let view = fixture.open()?;
    fixture
        .release
        .send(())
        .map_err(|error| error.to_string())?;
    let mut request = render_body(&view, 1);
    request["depthLimit"] = json!(2);
    request["viewport"]["width"] = json!(40);
    request["viewport"]["height"] = json!(40);
    request["minAreaPx2"] = json!(16);
    let rendered = fixture.ok("viz.render", request)?;
    let parsed: VizRenderResponse =
        serde_json::from_value(rendered.clone()).map_err(|error| error.to_string())?;
    parsed.validate()?;

    let model = &rendered["model"];
    let shapes = model["shapes"].as_array().ok_or("no shapes")?;
    let groups: Vec<_> = shapes
        .iter()
        .filter(|shape| shape["groupRef"].is_string())
        .collect();
    assert_eq!(groups.len(), 2, "expected a root group and a nested group");
    let root_group = groups
        .iter()
        .find(|shape| shape["visibleDepth"] == 1)
        .ok_or("no root group shape")?;
    let nested_group = groups
        .iter()
        .find(|shape| shape["visibleDepth"] == 2)
        .ok_or("no nested group shape")?;
    let count = |value: &Value| -> Result<u64, String> {
        value["value"]
            .as_str()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| format!("not a count: {value}"))
    };
    let root_member_count = count(&root_group["memberCount"])?;
    let nested_member_count = count(&nested_group["memberCount"])?;
    assert_eq!(root_member_count, 40);
    assert_eq!(nested_member_count, 100);
    assert_eq!(
        count(&model["summary"]["groupedItems"])?,
        root_member_count + nested_member_count
    );

    let mut all_page_totals = 0_u64;
    let mut group_members = Vec::new();
    for group in [root_group, nested_group] {
        let page = fixture.ok(
            "viz.group_rows",
            json!({"modelRef": model["modelRef"], "groupRef": group["groupRef"], "start": 0, "count": 1000}),
        )?;
        let member_count = count(&group["memberCount"])?;
        assert_eq!(page["totalCount"].as_u64(), Some(member_count));
        assert_eq!(
            page["rows"].as_array().map(Vec::len),
            Some(member_count as usize)
        );
        all_page_totals += page["totalCount"].as_u64().ok_or("invalid totalCount")?;
        group_members.push(
            page["rows"]
                .as_array()
                .ok_or("no group rows")?
                .iter()
                .map(|row| row["itemRef"].clone())
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(all_page_totals, count(&model["summary"]["groupedItems"])?);
    assert!(
        group_members[0]
            .iter()
            .all(|member| !group_members[1].contains(member))
    );
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

// Folder p holds a large file, an entry that could not be read, and forty small folders
// that each hold a file and an entry that could not be read, so every one of them is a
// lower bound; folder q holds one large file.
fn publish_unread_tail(sink: &IngestSink) -> Result<(), String> {
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
    let path = |parts: &[&str]| -> Vec<Vec<u8>> {
        parts.iter().map(|part| part.as_bytes().to_vec()).collect()
    };
    let exact = |bytes: u64| SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    };
    let failed = |error: du_analysis_index::BuildError| format!("{error:?}");
    scan.add_directory_at(0, vec![]).map_err(failed)?;
    scan.add_directory_at(0, path(&["p"])).map_err(failed)?;
    scan.add_file_at(
        0,
        Observation {
            path: path(&["p", "big"]),
            size: Some(exact(100_000)),
        },
    )
    .map_err(failed)?;
    scan.add_file_at(
        0,
        Observation {
            path: path(&["q"]),
            size: Some(exact(50_000)),
        },
    )
    .map_err(failed)?;
    scan.add_at(
        0,
        Observation {
            path: path(&["p", "gone"]),
            size: None,
        },
    )
    .map_err(failed)?;
    for i in 0..40 {
        let name = format!("s{i:02}");
        scan.add_directory_at(0, path(&["p", &name]))
            .map_err(failed)?;
        scan.add_file_at(
            0,
            Observation {
                path: path(&["p", &name, "f"]),
                size: Some(exact(500)),
            },
        )
        .map_err(failed)?;
        scan.add_at(
            0,
            Observation {
                path: path(&["p", &name, "u"]),
                size: None,
            },
        )
        .map_err(failed)?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(failed)?;
    Ok(())
}

#[test]
fn the_group_of_lower_bound_folders_inside_an_expanded_folder_is_read_by_position()
-> Result<(), String> {
    let sink = IngestSink::default();
    publish_unread_tail(&sink)?;
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
    let rendered = fixture.ok("viz.render", request)?;
    let parsed: VizRenderResponse =
        serde_json::from_value(rendered.clone()).map_err(|e| e.to_string())?;
    parsed.validate()?;
    let model = &rendered["model"];
    let shapes = model["shapes"].as_array().ok_or("no shapes")?;
    let group = shapes
        .iter()
        .find(|shape| shape["groupRef"].is_string())
        .ok_or_else(|| format!("no group shape: {}", model["summary"]))?;
    assert_eq!(group["visibleDepth"], 2);
    // Both folders are drawn by their lower bound; the group is a lower bound too, and
    // lies inside its folder's bytes, so it is no part of the grouped total.
    assert_eq!(group["size"]["state"], "lower_bound");
    assert_eq!(group["size"]["bytes"], "20000");
    assert_eq!(model["totals"]["grouped"]["quantity"]["value"], "0");
    assert_eq!(model["totals"]["scope"]["quantity"]["state"], "lower_bound");
    // The forty small folders are the members of the group.
    assert_eq!(model["summary"]["groupedItems"]["value"], "40");
    let members = fixture.ok(
        "viz.group_rows",
        json!({"modelRef": model["modelRef"], "groupRef": group["groupRef"], "start": 0, "count": 1000}),
    )?;
    assert_eq!(members["totalCount"], 40);
    assert_eq!(members["groupTotal"], group["size"]);
    assert_eq!(members["sizeRange"]["kind"], "unknown");
    // The folder's own table lists the large file and the group, each by its reference;
    // the group row is a lower bound and still counts its members exactly.
    let count = json!({"unit": "items", "state": "exact", "value": "40"});
    assert_eq!(group["memberCount"], count);
    assert_eq!(model["summary"]["groupedItems"], count);
    let folder = fixture.ok(
        "viz.rows",
        json!({"modelRef": model["modelRef"], "resultRevision": id(3), "start": 0, "count": 10, "parentShapeRef": group["parentShapeRef"]}),
    )?;
    assert_eq!(folder["totalCount"], 2);
    let group_row = folder["rows"]
        .as_array()
        .ok_or("no folder rows")?
        .iter()
        .find(|row| row["groupRef"] == group["groupRef"])
        .ok_or("no group row in the folder table")?;
    assert_eq!(group_row["size"], group["size"]);
    assert_eq!(group_row["memberCount"], count);
    let rows = members["rows"].as_array().ok_or("no member rows")?;
    assert!(rows.iter().all(|row| {
        row["size"]["state"] == "lower_bound"
            && row.get("labelKey").is_none()
            && row["groupRef"].is_null()
    }));
    Ok(())
}
