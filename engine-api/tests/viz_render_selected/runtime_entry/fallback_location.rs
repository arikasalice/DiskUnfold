use super::*;
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
    let a = item(&fixture.sink, "a")?;
    let x = item(&fixture.sink, "x")?;
    let outside = item(&fixture.sink, "outside")?;
    let fallback = fixture.ok("viz.render", scope(&view, 2, &a))?;
    assert_eq!(fallback["kind"], "unavailable");
    assert_eq!(fallback["reasonKey"], "viz.unavailable.no_attribution");
    let model = fallback["fallback"]["modelRef"].clone();
    let rows = fixture.ok(
        "viz.rows",
        json!({"modelRef":model,"resultRevision":id(3),"start":0,"count":10}),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_eq!(rows["totalCount"], 2);
    for row in rows["rows"].as_array().ok_or("missing rows")? {
        assert_eq!(row["size"]["bytes"], "50");
        assert_eq!(row["denominator"]["size"]["bytes"], "100");
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
        "result.requery_required"
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
    let returned = fixture.ok("viz.render", scope(&view, 4, &a))?;
    assert_eq!(returned["fallback"]["modelRef"], model);
    Ok(())
}

#[test]
fn missing_order_evidence_returns_same_revision_fallback() -> Result<(), String> {
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
    assert_eq!(result["kind"], "unavailable");
    assert_eq!(result["fallback"]["resultRevision"], json!(id(3)));
    assert_eq!(fixture.ok("viz.rows",json!({"modelRef":result["fallback"]["modelRef"],"resultRevision":id(3),"start":0,"count":10}))?["totalCount"], 2);
    Ok(())
}
