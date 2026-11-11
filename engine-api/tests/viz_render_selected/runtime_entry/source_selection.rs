use super::*;

struct FixedSelection(Option<Id>);

impl du_engine_api::viz_runtime::selection::WindowScanSelection for FixedSelection {
    fn selected_result_revision(
        &self,
        _view_channel: &Id,
    ) -> Result<Option<Id>, du_engine_api::viz_runtime::selection::SelectionError> {
        Ok(self.0.clone())
    }
}

fn publish_other_lineage(sink: &du_analysis_index::IngestSink) -> Result<(), String> {
    use du_core_contracts::{
        PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
        result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    };

    let lineage = sink
        .bind_current_analysis("other-window-scope", id(8))
        .map_err(|error| format!("{error:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/other"),
        },
        volume_id: vec![8],
        file_id: vec![9],
    };
    let mut build = sink
        .open_forest_in_lineage(id(9), vec![root], lineage)
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_file(du_analysis_index::Observation {
            path: vec![b"other".to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact { bytes: 900.into() },
            }),
        })
        .map_err(|error| format!("{error:?}"))?;
    build
        .seal(id(4), ResultCompleteness::Complete)
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
}

fn assert_shapes_match_rows(model: &Value, rows: &Value, revision: &Id) -> Result<(), String> {
    let shapes = model["shapes"].as_array().ok_or("missing treemap shapes")?;
    assert_eq!(shapes.len(), 2);
    let row_values = rows["rows"].as_array().ok_or("missing treemap rows")?;
    for shape in shapes {
        assert_eq!(shape["itemRef"]["resultVersion"], json!(revision));
        assert_eq!(shape["groupRef"], Value::Null);
        assert_eq!(shape["size"]["evidence"], json!("measured"));
        let row = row_values
            .iter()
            .find(|row| row["itemRef"] == shape["itemRef"])
            .ok_or("shape has no matching row")?;
        assert_eq!(row["size"], shape["size"]);
        assert_eq!(row["groupRef"], Value::Null);
    }
    Ok(())
}

fn row_size_bytes(rows: &Value) -> Result<Vec<String>, String> {
    rows["rows"]
        .as_array()
        .ok_or("missing treemap rows")?
        .iter()
        .map(|row| {
            assert_eq!(row["itemRef"]["resultVersion"], json!(id(3)));
            assert_eq!(row["size"]["basis"], json!("logical"));
            assert_eq!(row["size"]["evidence"], json!("measured"));
            row["size"]["bytes"]
                .as_str()
                .ok_or_else(|| "missing size bytes".to_owned())
                .map(str::to_owned)
        })
        .collect()
}

#[test]
fn different_rule_evidence_is_rejected_before_admitting_render_work() -> Result<(), String> {
    let fixture = Fixture::with_wait([25, 75], Duration::from_secs(2))?;
    let view = fixture.open()?;
    for criterion in ["fixture.bad_rule_id", "fixture.bad_rule_revision"] {
        let mut request = render_body(&view, 1);
        request["classificationCriterion"] = json!(criterion);
        let error = fixture
            .call("viz.render", request)
            .error
            .ok_or("mismatched aggregation rule was accepted")?;
        assert_eq!(error.reason_key, "support.internal_error");
        assert!(fixture.entered.try_recv().is_err());
    }

    fixture.release.send(()).map_err(|e| e.to_string())?;
    let rendered = fixture.render(&view, 1)?;
    assert_eq!(rendered["kind"], "model");
    let version = fixture
        .sink
        .pin_current(&id(3))
        .map_err(|e| format!("{e:?}"))?;
    let applied = version.size_rule();
    assert_eq!(
        rendered["model"]["tableRevisions"]["sizeRule"],
        json!({"id": applied.id, "revision": applied.revision})
    );
    Ok(())
}

#[test]
fn omitted_revision_selects_current_scan_but_explicit_revision_cannot_drift() -> Result<(), String>
{
    let fixture = Fixture::new([25, 75])?;
    let view = fixture.open()?;
    publish(&fixture.sink, id(2), id(4))?;
    let mut request = render_body(&view, 1);
    request["sourceRef"] = json!({"kind": "scan", "ref": id(1)});
    assert_eq!(
        fixture
            .call("viz.render", request.clone())
            .error
            .ok_or("stale explicit result was substituted")?
            .reason_key,
        "result.version_stale"
    );
    request["sourceRef"]["ref"] = json!(id(9));
    assert_eq!(
        fixture
            .call("viz.render", request.clone())
            .error
            .ok_or("unknown lineage was substituted")?
            .reason_key,
        "result.version_not_found"
    );
    request["sourceRef"]["ref"] = json!(id(1));
    request
        .as_object_mut()
        .ok_or("invalid fixture request")?
        .remove("resultRevision");
    let root = json!({"resultVersion": id(4), "itemNo": 0});
    request["aggregateScopeRef"] = root.clone();
    request["navigationScopeRef"] = root;
    let response = fixture.ok("viz.render", request)?;
    assert_eq!(response["resultRevision"], json!(id(4)));
    let job_ref = job(&response)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|error| error.to_string())?;
    fixture
        .release
        .send(())
        .map_err(|error| error.to_string())?;
    fixture.finished(&job_ref, JobState::Done)
}

#[test]
fn injected_window_selection_pins_its_scan_through_treemap_model_and_rows() -> Result<(), String> {
    let sink = du_analysis_index::IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 75])?;
    publish_other_lineage(&sink)?;
    let fixture = super::histogram_strategy::fixture_with_selection(
        sink.clone(),
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
        super::histogram_runtime::dispatcher()?,
        Arc::new(FixedSelection(Some(id(3)))),
    )?;
    let view = fixture.open()?;
    fixture
        .release
        .send(())
        .map_err(|error| error.to_string())?;

    let mut request = render_body(&view, 1);
    assert_eq!(request["format"], json!("treemap"));
    request
        .as_object_mut()
        .ok_or("invalid fixture request")?
        .remove("resultRevision");
    let rendered = fixture.ok("viz.render", request.clone())?;
    assert_eq!(rendered["resultRevision"], json!(id(3)));
    assert_eq!(
        rendered["model"]["aggregateScopeRef"]["resultVersion"],
        json!(id(3))
    );
    let selected = sink
        .pin_current(&id(3))
        .map_err(|error| format!("{error:?}"))?;
    let rule = selected.size_rule();
    let raw_size = selected
        .items()
        .get(1)
        .and_then(|item| item.size.as_ref())
        .ok_or("selected scan has no raw file size evidence")?;
    assert_eq!(raw_size.evidence, du_core_contracts::SizeEvidence::Measured);
    assert_eq!(
        raw_size.amount,
        du_core_contracts::SizeAmount::Exact { bytes: 25.into() }
    );
    assert_eq!(
        rendered["model"]["tableRevisions"]["sizeRule"],
        json!({"id": rule.id, "revision": rule.revision})
    );
    assert_eq!(
        rendered["model"]["strategy"],
        json!({"id":"viz.treemap","revision":1})
    );
    assert_eq!(
        rendered["model"]["representation"]["format"],
        json!("treemap")
    );
    assert_eq!(
        rendered["model"]["totals"]["scope"]["sourceSize"]["bytes"],
        json!("100")
    );
    assert_eq!(rule.id, "size.agg.subtree-unique");
    assert_eq!(rule.revision, 1);

    let model_ref = rendered["model"]["modelRef"]
        .as_str()
        .ok_or("missing model reference")?;
    let rows = fixture.ok(
        "viz.rows",
        json!({"modelRef":model_ref,"resultRevision":id(3),"start":0,"count":10}),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_shapes_match_rows(&rendered["model"], &rows, &id(3))?;
    let mut sizes = row_size_bytes(&rows)?;
    sizes.sort();
    assert_eq!(sizes, vec!["25", "75"]);

    let mut mismatched_revision = request.clone();
    mismatched_revision["resultRevision"] = json!(id(4));
    assert_eq!(
        fixture
            .call("viz.render", mismatched_revision)
            .error
            .ok_or("request revision different from window selection was accepted")?
            .reason_key,
        "result.version_stale"
    );
    let mut mismatched_lineage = request;
    mismatched_lineage["sourceRef"] = json!({"kind":"scan","ref":id(8)});
    assert_eq!(
        fixture
            .call("viz.render", mismatched_lineage)
            .error
            .ok_or("request lineage different from window selection was accepted")?
            .reason_key,
        "result.version_stale"
    );
    Ok(())
}

#[test]
fn injected_window_selection_fails_closed_without_a_selected_source() -> Result<(), String> {
    let sink = du_analysis_index::IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 75])?;
    let fixture = super::histogram_strategy::fixture_with_selection(
        sink,
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
        super::histogram_runtime::dispatcher()?,
        Arc::new(FixedSelection(None)),
    )?;
    let view = fixture.open()?;
    let mut request = render_body(&view, 1);
    request
        .as_object_mut()
        .ok_or("invalid fixture request")?
        .remove("resultRevision");
    assert_eq!(
        fixture
            .call("viz.render", request)
            .error
            .ok_or("render without a selected source was accepted")?
            .reason_key,
        "ref.unavailable"
    );
    assert!(fixture.entered.try_recv().is_err());
    Ok(())
}

#[test]
fn injected_window_selection_does_not_replace_a_stale_selection() -> Result<(), String> {
    let sink = du_analysis_index::IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 75])?;
    let fixture = super::histogram_strategy::fixture_with_selection(
        sink.clone(),
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
        super::histogram_runtime::dispatcher()?,
        Arc::new(FixedSelection(Some(id(3)))),
    )?;
    let view = fixture.open()?;
    publish_sizes(&sink, id(2), id(4), [25, 75])?;
    let result = fixture.call("viz.render", render_body(&view, 1));
    assert_eq!(
        result
            .error
            .ok_or("stale selected revision was substituted")?
            .reason_key,
        "result.version_stale"
    );
    assert!(fixture.entered.try_recv().is_err());
    Ok(())
}

struct UnavailableSelection;
impl du_engine_api::viz_runtime::selection::WindowScanSelection for UnavailableSelection {
    fn selected_result_revision(
        &self,
        _: &Id,
    ) -> Result<Option<Id>, du_engine_api::viz_runtime::selection::SelectionError> {
        Err(du_engine_api::viz_runtime::selection::SelectionError::Unavailable)
    }
}

#[test]
fn selection_provider_failure_does_not_fall_back_to_latest() -> Result<(), String> {
    let sink = du_analysis_index::IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 75])?;
    let fixture = super::histogram_strategy::fixture_with_selection(
        sink,
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
        super::histogram_runtime::dispatcher()?,
        Arc::new(UnavailableSelection),
    )?;
    let view = fixture.open()?;
    let result = fixture.call("viz.render", render_body(&view, 1));
    assert_eq!(
        result
            .error
            .ok_or("selection failure was ignored")?
            .reason_key,
        "support.internal_error"
    );
    assert!(fixture.entered.try_recv().is_err());
    Ok(())
}

struct MutableSelection(Mutex<Option<Id>>);
impl du_engine_api::viz_runtime::selection::WindowScanSelection for MutableSelection {
    fn selected_result_revision(
        &self,
        _: &Id,
    ) -> Result<Option<Id>, du_engine_api::viz_runtime::selection::SelectionError> {
        self.0
            .lock()
            .map(|selected| selected.clone())
            .map_err(|_| du_engine_api::viz_runtime::selection::SelectionError::Unavailable)
    }
}

#[test]
fn owner_bound_rule_uses_only_the_selected_pinned_result() -> Result<(), String> {
    let sink = du_analysis_index::IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 75])?;
    let selected = Arc::new(MutableSelection(Mutex::new(None)));
    let fixture = super::histogram_strategy::fixture_with_selection(
        sink.clone(),
        Duration::from_secs(2),
        NonZeroUsize::MIN.saturating_add(3),
        super::histogram_runtime::dispatcher()?,
        selected.clone(),
    )?;
    let view = fixture.open()?;
    let mut request = render_body(&view, 1);
    request["classificationCriterion"] = json!("fixture.owner_rule");
    assert_eq!(
        fixture
            .call("viz.render", request.clone())
            .error
            .ok_or("missing selection accepted")?
            .reason_key,
        "ref.unavailable"
    );
    *selected.0.lock().map_err(|e| e.to_string())? = Some(id(3));
    let mut missing_policy = request.clone();
    missing_policy["classificationCriterion"] = json!("fixture.owner_missing_policy");
    assert_eq!(
        fixture
            .call("viz.render", missing_policy)
            .error
            .ok_or("missing policy accepted")?
            .reason_key,
        "support.internal_error"
    );
    assert!(fixture.entered.try_recv().is_err());
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let first = fixture.ok("viz.render", request.clone())?;
    let first_rule = sink
        .pin_current(&id(3))
        .map_err(|e| format!("{e:?}"))?
        .size_rule();
    assert_eq!(
        first["model"]["tableRevisions"]["sizeRule"],
        json!({"id":first_rule.id,"revision":first_rule.revision})
    );
    publish_sizes(&sink, id(2), id(4), [40, 60])?;
    request["requestSeq"] = json!(2);
    assert_eq!(
        fixture
            .call("viz.render", request.clone())
            .error
            .ok_or("replaced revision accepted")?
            .reason_key,
        "result.version_stale"
    );
    *selected.0.lock().map_err(|e| e.to_string())? = Some(id(4));
    request["requestSeq"] = json!(3);
    request["resultRevision"] = json!(id(4));
    request["aggregateScopeRef"]["resultVersion"] = json!(id(4));
    request["navigationScopeRef"]["resultVersion"] = json!(id(4));
    let second = fixture.ok("viz.render", request)?;
    let second_rule = sink
        .pin_current(&id(4))
        .map_err(|e| format!("{e:?}"))?
        .size_rule();
    assert_eq!(second["resultRevision"], json!(id(4)));
    assert_eq!(
        second["model"]["tableRevisions"]["sizeRule"],
        json!({"id":second_rule.id,"revision":second_rule.revision})
    );
    assert_ne!(first["model"]["modelRef"], second["model"]["modelRef"]);
    Ok(())
}

pub(super) fn owner_policy(request: &VizRenderRequest) -> Option<SourcePolicyFacts> {
    if !["fixture.owner_rule", "fixture.owner_missing_policy"]
        .contains(&request.classification_criterion.as_str())
    {
        return None;
    }
    Some(SourcePolicyFacts {
        staleness: VizStaleness {
            state: VizStalenessState::Current,
            reason_key: None,
        },
        interpretation_revision: 1,
        required_data_revision: 1,
        legend_revision: u32::from(request.classification_criterion == "fixture.owner_rule"),
        view_policy_revision: 1,
        classes: HashMap::from([(
            "fixture.class".into(),
            LegendClass {
                label_key: "fixture.label".into(),
                condition: None,
            },
        )]),
        available_formats: vec![],
    })
}
