use super::*;
use du_analysis_index::{FileIdentity, SharingEvidence, SpaceObservation};
fn windows_name(name: &str) -> Vec<u8> {
    name.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn tied_tree() -> Result<IngestSink, String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|e| format!("{e:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&windows_name("C:\\fixture")),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut scan = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|e| format!("{e:?}"))?;
    for path in [
        vec![],
        vec![windows_name("a")],
        vec![windows_name("a"), windows_name("x")],
        vec![windows_name("a"), windows_name("y")],
        vec![windows_name("outside")],
    ] {
        scan.add_directory(path).map_err(|e| format!("{e:?}"))?;
    }
    for (path, size) in [
        (vec!["a", "x", "one"], 50_u64),
        (vec!["a", "y", "two"], 50),
        (vec!["outside", "three"], 25),
    ] {
        scan.add_file(Observation {
            path: path.into_iter().map(windows_name).collect(),
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact { bytes: size.into() },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok(sink)
}
/// Like `tied_tree`, but the files below `a/x` and `a/y` are two links of one
/// file: only the allocated basis cannot attribute them to either sibling.
fn shared_link_tree() -> Result<IngestSink, String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|e| format!("{e:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&windows_name("C:\\fixture")),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut scan = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|e| format!("{e:?}"))?;
    for path in [
        vec![],
        vec![windows_name("a")],
        vec![windows_name("a"), windows_name("x")],
        vec![windows_name("a"), windows_name("y")],
        vec![windows_name("outside")],
    ] {
        scan.add_directory(path).map_err(|e| format!("{e:?}"))?;
    }
    for (path, size, file, links) in [
        (vec!["a", "x", "one"], 50_u64, 7_u8, 2_u64),
        (vec!["a", "y", "two"], 50, 7, 2),
        (vec!["outside", "three"], 25, 8, 1),
    ] {
        let amount = |basis, evidence| SizeValue {
            basis,
            evidence,
            amount: SizeAmount::Exact { bytes: size.into() },
        };
        scan.add_with_space(
            Observation {
                path: path.into_iter().map(windows_name).collect(),
                size: Some(amount(SizeBasis::Logical, SizeEvidence::Measured)),
            },
            SpaceObservation {
                allocated: Some(amount(SizeBasis::Allocated, SizeEvidence::OsReported)),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![file],
                }),
                link_count: Some(links),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok(sink)
}
fn allocated(mut request: Value) -> Value {
    request["sizeBasis"] = json!("allocated");
    request
}
fn item(sink: &IngestSink, name: &str) -> Result<Value, String> {
    let version = sink.pin_current(&id(3)).map_err(|e| format!("{e:?}"))?;
    let value = version
        .items()
        .iter()
        .find(|item| item.name == windows_name(name))
        .ok_or("missing item")?;
    Ok(json!({"resultVersion":id(3),"itemNo":value.item_no}))
}
fn scope(view: &Value, seq: u32, parent: &Value) -> Value {
    let mut request = render_body(view, seq);
    request["aggregateScopeRef"] = parent.clone();
    request["navigationScopeRef"] = parent.clone();
    request
}
#[test]
fn fallback_selection_location_and_folder_navigation_keep_the_pinned_source() -> Result<(), String>
{
    let fixture = Fixture::from_sink(
        shared_link_tree()?,
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
    )?;
    let view = fixture.open()?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    assert_eq!(
        fixture.ok("viz.render", render_body(&view, 1))?["kind"],
        "model"
    );
    let a = item(&fixture.sink, "a")?;
    let x = item(&fixture.sink, "x")?;
    let outside = item(&fixture.sink, "outside")?;
    let fallback = fixture.ok("viz.render", allocated(scope(&view, 2, &a)))?;
    assert_eq!(fallback["kind"], "unavailable");
    assert_eq!(fallback["reasonKey"], "viz.unavailable.no_attribution");
    let model = fallback["fallback"]["modelRef"].clone();
    let rows = fixture.ok(
        "viz.rows",
        json!({"modelRef":model,"resultRevision":id(3),"start":0,"count":10}),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_eq!(rows["totalCount"], 2);
    // The one file under `a` is linked from both children, so `a` counts it once.
    for row in rows["rows"].as_array().ok_or("missing rows")? {
        assert_eq!(row["size"]["bytes"], "50");
        assert_eq!(row["denominator"]["size"]["bytes"], "50");
        assert_eq!(row["itemRef"]["resultVersion"], json!(id(3)));
    }
    assert_eq!(
        fixture.ok("viz.locate", json!({"modelRef":model,"itemRef":x}))?["location"],
        "outside_view"
    );
    assert_eq!(
        fixture.ok("viz.locate", json!({"modelRef":model,"itemRef":outside}))?["location"],
        "outside_scope"
    );
    let mut wrong = x.clone();
    wrong["resultVersion"] = json!(id(4));
    assert_eq!(
        fixture
            .call("viz.locate", json!({"modelRef":model,"itemRef":wrong}))
            .error
            .ok_or("stale accepted")?
            .reason_key,
        "result.version_stale"
    );
    assert_eq!(
        fixture
            .call(
                "viz.locate",
                json!({"modelRef":model,"itemRef":{"resultVersion":id(3),"itemNo":u32::MAX}})
            )
            .error
            .ok_or("unknown item accepted")?
            .reason_key,
        "ref.unavailable"
    );
    let child = fixture.ok("viz.render", scope(&view, 3, &x))?;
    assert_eq!(child["kind"], "model");
    assert_eq!(child["resultRevision"], json!(id(3)));
    assert_eq!(
        child["model"]["totals"]["scope"]["sourceSize"]["bytes"],
        "50"
    );
    let returned = fixture.ok("viz.render", allocated(scope(&view, 4, &a)))?;
    assert_eq!(returned["fallback"]["modelRef"], model);
    Ok(())
}

#[test]
fn equal_size_folder_children_render_a_model_in_name_order() -> Result<(), String> {
    let fixture = Fixture::from_sink(
        tied_tree()?,
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
    )?;
    let view = fixture.open()?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    assert_eq!(
        fixture.ok("viz.render", render_body(&view, 1))?["kind"],
        "model"
    );
    // The producer supplies no comparison key; this tie used to leave only a table.
    let a = item(&fixture.sink, "a")?;
    let model = fixture.ok("viz.render", scope(&view, 2, &a))?;
    assert_eq!(model["kind"], "model");
    assert_eq!(model["resultRevision"], json!(id(3)));
    let rows = fixture.ok(
        "viz.rows",
        json!({"modelRef":model["model"]["modelRef"],"resultRevision":id(3),"start":0,"count":10}),
    )?;
    let x = item(&fixture.sink, "x")?;
    let y = item(&fixture.sink, "y")?;
    let shown: Vec<_> = model["model"]["shapes"]
        .as_array()
        .ok_or("missing shapes")?
        .iter()
        .map(|shape| shape["itemRef"].clone())
        .collect();
    assert_eq!(shown, [x.clone(), y.clone()]);
    let listed: Vec<_> = rows["rows"]
        .as_array()
        .ok_or("missing rows")?
        .iter()
        .map(|row| row["itemRef"].clone())
        .collect();
    assert_eq!(listed, [x, y]);
    Ok(())
}

#[test]
fn equal_sizes_without_order_evidence_render_a_model_in_name_order() -> Result<(), String> {
    let fixture = Fixture::new([50, 50])?;
    let a = fixture.open()?;
    let first = job(&fixture.render(&a, 1)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    fixture.finished(&first, JobState::Done)?;
    let result = fixture.render(&a, 2)?;
    assert_eq!(result["kind"], "model");
    assert_eq!(result["resultRevision"], json!(id(3)));
    let rows = fixture.ok(
        "viz.rows",
        json!({"modelRef":result["model"]["modelRef"],"resultRevision":id(3),"start":0,"count":10}),
    )?;
    assert_eq!(rows["totalCount"], 2);
    let names: Vec<_> = rows["rows"]
        .as_array()
        .ok_or("missing rows")?
        .iter()
        .map(|row| row["displayName"].clone())
        .collect();
    assert_eq!(names, [json!("a"), json!("b")]);
    assert_eq!(result["model"]["shapes"].as_array().map(Vec::len), Some(2));
    Ok(())
}

#[test]
fn unknown_sizes_keep_the_same_revision_no_attribution_fallback() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|e| format!("{e:?}"))?;
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
        .map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for name in [b"a", b"b"] {
        scan.add_file(Observation {
            path: vec![name.to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    scan.publish_partial(id(3)).map_err(|e| format!("{e:?}"))?;
    let fixture = Fixture::from_sink(
        sink,
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
    )?;
    let view = fixture.open()?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let result = fixture.render(&view, 1)?;
    assert_eq!(result["kind"], "unavailable");
    assert_eq!(result["reasonKey"], "viz.unavailable.no_attribution");
    assert_eq!(result["fallback"]["resultRevision"], json!(id(3)));
    let rows = fixture.ok("viz.rows",json!({"modelRef":result["fallback"]["modelRef"],"resultRevision":id(3),"start":0,"count":10}))?;
    assert_eq!(rows["totalCount"], 2);
    Ok(())
}
