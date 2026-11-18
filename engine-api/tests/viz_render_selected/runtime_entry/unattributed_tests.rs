use super::*;
use du_analysis_index::{FileIdentity, SharingEvidence, SpaceObservation};
use du_core_contracts::visualization::response::VizRenderResponse;

// Folders a and b hold the same file (two links); folder c holds an independent one.
fn publish_shared(sink: &IngestSink) -> Result<(), String> {
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
    scan.add_directory_at(0, vec![])
        .map_err(|error| format!("{error:?}"))?;
    for name in ["a", "b", "c"] {
        scan.add_directory_at(0, vec![name.as_bytes().to_vec()])
            .map_err(|error| format!("{error:?}"))?;
    }
    for (name, file, links) in [("a", 7_u8, 2_u64), ("b", 7, 2), ("c", 9, 1)] {
        scan.add_with_space_at(
            0,
            Observation {
                path: vec![name.as_bytes().to_vec(), b"file".to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact { bytes: 10.into() },
                }),
            },
            SpaceObservation {
                allocated: Some(SizeValue {
                    basis: SizeBasis::Allocated,
                    evidence: SizeEvidence::OsReported,
                    amount: SizeAmount::Exact { bytes: 10.into() },
                }),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![file],
                }),
                link_count: Some(links),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

fn publish_nested_shared(sink: &IngestSink) -> Result<(), String> {
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
    scan.add_directory_at(0, vec![])
        .map_err(|error| format!("{error:?}"))?;
    scan.add_directory_at(0, vec![b"p".to_vec()])
        .map_err(|error| format!("{error:?}"))?;
    for name in ["x", "y"] {
        scan.add_directory_at(0, vec![b"p".to_vec(), name.as_bytes().to_vec()])
            .map_err(|error| format!("{error:?}"))?;
        scan.add_with_space_at(
            0,
            Observation {
                path: vec![b"p".to_vec(), name.as_bytes().to_vec(), b"file".to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact { bytes: 10.into() },
                }),
            },
            SpaceObservation {
                allocated: Some(SizeValue {
                    basis: SizeBasis::Allocated,
                    evidence: SizeEvidence::OsReported,
                    amount: SizeAmount::Exact { bytes: 10.into() },
                }),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![7],
                }),
                link_count: Some(2),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

#[test]
fn siblings_without_a_provable_area_are_counted_and_the_class_total_is_a_lower_bound()
-> Result<(), String> {
    let sink = IngestSink::default();
    publish_shared(&sink)?;
    let fixture = Fixture::from_sink(
        sink,
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
    )?;
    let view = fixture.open()?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let mut request = render_body(&view, 1);
    request["sizeBasis"] = json!("allocated");
    let rendered = fixture.ok("viz.render", request)?;
    assert_eq!(rendered["kind"], "model");
    let parsed: VizRenderResponse =
        serde_json::from_value(rendered.clone()).map_err(|e| e.to_string())?;
    parsed.validate()?;
    let model = &rendered["model"];
    // Only the independent folder has a shape; the two sharing folders are counted.
    assert_eq!(model["shapes"].as_array().ok_or("no shapes")?.len(), 1);
    assert_eq!(model["summary"]["unknownItems"]["value"], "2");
    assert_eq!(model["summary"]["groupedItems"]["value"], "0");
    assert_eq!(model["summary"]["reasonKeys"], json!([]));
    let totals = &model["totals"];
    assert_eq!(totals["scope"]["quantity"]["value"], "20");
    assert_eq!(totals["drawn"]["quantity"]["value"], "10");
    assert_eq!(totals["grouped"]["quantity"]["value"], "0");
    assert_eq!(totals["filteredOut"]["quantity"]["value"], "0");
    // The 10 bytes in no total are the shared file, which the scope counts once. The two
    // siblings that contain it list 10 each in the table, and those are never added.
    let wire = |name: &str| -> Result<u64, String> {
        totals[name]["quantity"]["value"]
            .as_str()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| format!("no {name} total"))
    };
    assert!(wire("drawn")? + wire("grouped")? <= wire("scope")?);
    assert_eq!(
        wire("scope")? - wire("drawn")? - wire("grouped")? - wire("filteredOut")?,
        10
    );
    // Counted but never added: the class has three items and only a lower-bound total.
    let legend = &model["legend"][0];
    assert_eq!(legend["itemCount"]["value"], "3");
    assert_eq!(legend["total"]["quantity"]["state"], "lower_bound");
    assert_eq!(legend["total"]["quantity"]["value"], "10");
    // The table keeps all three rows with their original values.
    let rows = fixture.ok(
        "viz.rows",
        json!({"modelRef": model["modelRef"], "resultRevision": id(3), "start": 0, "count": 10}),
    )?;
    assert_eq!(rows["totalCount"], 3);
    Ok(())
}

#[test]
fn expanding_only_unattributed_children_moves_them_from_omitted_to_unknown() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_nested_shared(&sink)?;
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

    let mut initial_request = render_body(&view, 1);
    initial_request["sizeBasis"] = json!("allocated");
    let initial = fixture.ok("viz.render", initial_request)?;
    assert_eq!(initial["kind"], "model");
    let parent = initial["model"]["shapes"]
        .as_array()
        .and_then(|shapes| shapes.first())
        .ok_or("missing parent shape")?["itemRef"]
        .clone();
    assert_eq!(initial["model"]["summary"]["omittedItems"]["value"], "2");
    assert_eq!(initial["model"]["summary"]["unknownItems"]["value"], "0");

    let mut expanded_request = render_body(&view, 2);
    expanded_request["sizeBasis"] = json!("allocated");
    expanded_request["expandedDepthByItem"] = json!([{"itemRef": parent, "extraDepth": 1}]);
    let expanded = fixture.ok("viz.render", expanded_request)?;
    assert_eq!(expanded["kind"], "model");
    assert_eq!(expanded["model"]["summary"]["omittedItems"]["value"], "0");
    assert_eq!(expanded["model"]["summary"]["unknownItems"]["value"], "2");
    Ok(())
}
