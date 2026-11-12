use super::*;

fn size_less_fixture(wait: Duration) -> Result<Fixture, String> {
    let sink = IngestSink::default();
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
    scan.add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    for name in b"ab".iter().copied() {
        scan.add_file(Observation {
            path: vec![vec![name]],
            size: None,
        })
        .map_err(|error| format!("{error:?}"))?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Fixture::from_sink(sink, wait, NonZeroUsize::MIN.saturating_add(3))
}

fn empty_fixture(wait: Duration) -> Result<Fixture, String> {
    let sink = IngestSink::default();
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
    scan.add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Fixture::from_sink(sink, wait, NonZeroUsize::MIN.saturating_add(3))
}

fn histogram_body(view: &Value, seq: u32) -> Value {
    let mut body = render_body(view, seq);
    body["format"] = json!("histogram");
    body
}

#[test]
fn opt_in_required_data_uses_pinned_facts_and_same_revision_model_rows() -> Result<(), String> {
    let fixture = Fixture::with_wait([25, 75], Duration::from_secs(2))?;
    fixture
        .producer
        .required_data_mode
        .store(1, Ordering::Relaxed);
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let view = fixture.open()?;
    let first = fixture.render(&view, 1)?;
    let model = if first["kind"] == "job" {
        let physical = job(&first)?;
        fixture.finished(&physical, JobState::Done)?;
        fixture.render(&view, 2)?
    } else {
        first
    };
    assert_eq!(model["kind"], "model");
    let model = &model["model"];
    let rows = fixture.ok(
        "viz.rows",
        json!({
            "modelRef": model["modelRef"],
            "resultRevision": id(3),
            "start": 0,
            "count": 10
        }),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_eq!(rows["totalCount"], 2);
    Ok(())
}

#[test]
fn opted_in_missing_sizes_publish_only_same_revision_fallback_rows_sync_and_async()
-> Result<(), String> {
    for wait in [Duration::from_secs(2), Duration::ZERO] {
        let fixture = size_less_fixture(wait)?;
        fixture
            .producer
            .required_data_mode
            .store(2, Ordering::Relaxed);
        fixture
            .release
            .send(())
            .map_err(|error| error.to_string())?;
        let view = fixture.open()?;
        let first = fixture.render(&view, 1)?;
        let unavailable = if first["kind"] == "job" {
            assert_eq!(wait, Duration::ZERO);
            let physical = job(&first)?;
            fixture.finished(&physical, JobState::Done)?;
            fixture.render(&view, 2)?
        } else {
            assert_eq!(wait, Duration::from_secs(2));
            first
        };
        assert_eq!(unavailable["kind"], "unavailable");
        assert_eq!(unavailable["reasonKey"], "viz.unavailable.no_item_sizes");
        assert_eq!(unavailable["availableFormats"], json!([]));
        let fallback = &unavailable["fallback"];
        assert_eq!(fallback["resultRevision"], json!(id(3)));
        let rows = fixture.ok(
            "viz.rows",
            json!({
                "modelRef": fallback["modelRef"],
                "resultRevision": fallback["resultRevision"],
                "start": 0,
                "count": 10
            }),
        )?;
        assert_eq!(rows["resultRevision"], json!(id(3)));
        assert_eq!(rows["totalCount"], 2);
    }
    Ok(())
}

#[test]
fn opt_in_unknown_evidence_fails_closed_before_job_submission() -> Result<(), String> {
    let fixture = Fixture::with_wait([25, 75], Duration::ZERO)?;
    fixture
        .producer
        .required_data_mode
        .store(3, Ordering::Relaxed);
    fixture
        .producer
        .hierarchy_unknown
        .store(true, Ordering::Relaxed);
    let view = fixture.open()?;
    let error = fixture
        .call("viz.render", render_body(&view, 1))
        .error
        .ok_or("unknown evidence was accepted")?;
    assert_eq!(error.reason_key, "support.internal_error");
    assert!(fixture.events.try_recv().is_err());
    Ok(())
}

#[test]
fn available_facts_preserve_the_existing_same_revision_ordering_fallback() -> Result<(), String> {
    let fixture = Fixture::with_wait([50, 50], Duration::from_secs(2))?;
    fixture
        .producer
        .required_data_mode
        .store(1, Ordering::Relaxed);
    fixture
        .release
        .send(())
        .map_err(|error| error.to_string())?;
    let view = fixture.open()?;
    let response = fixture.render(&view, 1)?;
    assert_eq!(response["kind"], "unavailable");
    assert_eq!(response["reasonKey"], "viz.unavailable.no_attribution");
    assert_eq!(response["availableFormats"], json!(["histogram"]));
    let fallback = &response["fallback"];
    let rows = fixture.ok(
        "viz.rows",
        json!({
            "modelRef": fallback["modelRef"],
            "resultRevision": fallback["resultRevision"],
            "start": 0,
            "count": 10
        }),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_eq!(rows["totalCount"], 2);
    Ok(())
}

#[test]
fn evaluated_requirements_are_part_of_runtime_deduplication_identity() -> Result<(), String> {
    let fixture = Fixture::with_wait([25, 75], Duration::from_secs(2))?;
    fixture
        .release
        .send(())
        .map_err(|error| error.to_string())?;
    let view = fixture.open()?;
    let legacy = fixture.render(&view, 1)?;
    assert_eq!(legacy["kind"], "model");
    fixture
        .producer
        .required_data_mode
        .store(1, Ordering::Relaxed);
    let opted_in = fixture.render(&view, 2)?;
    assert_eq!(opted_in["kind"], "model");
    assert_ne!(legacy["model"]["modelRef"], opted_in["model"]["modelRef"]);
    Ok(())
}

fn assert_missing_histogram_edges(fixture: Fixture, expected_rows: usize) -> Result<(), String> {
    fixture
        .producer
        .required_data_mode
        .store(1, Ordering::Relaxed);
    fixture
        .producer
        .histogram_bins_absent
        .store(true, Ordering::Relaxed);
    fixture
        .release
        .send(())
        .map_err(|error| error.to_string())?;
    let view = fixture.open()?;
    let response = fixture.ok("viz.render", histogram_body(&view, 1))?;
    assert_eq!(response["kind"], "unavailable");
    assert_eq!(response["reasonKey"], "viz.unavailable.no_item_sizes");
    assert_eq!(
        response["availableFormats"],
        if expected_rows > 0 {
            json!(["treemap"])
        } else {
            json!([])
        }
    );
    let fallback = &response["fallback"];
    assert_eq!(fallback["resultRevision"], json!(id(3)));
    let rows = fixture.ok(
        "viz.rows",
        json!({
            "modelRef": fallback["modelRef"],
            "resultRevision": fallback["resultRevision"],
            "start": 0,
            "count": 10
        }),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_eq!(rows["totalCount"], expected_rows);
    assert_eq!(rows["rows"].as_array().map(Vec::len), Some(expected_rows));
    Ok(())
}

#[test]
fn missing_histogram_edges_keep_rows_from_the_pinned_source() -> Result<(), String> {
    assert_missing_histogram_edges(Fixture::with_wait([25, 75], Duration::from_secs(2))?, 2)
}

#[test]
fn missing_histogram_edges_keep_a_valid_zero_row_model() -> Result<(), String> {
    assert_missing_histogram_edges(empty_fixture(Duration::from_secs(2))?, 0)
}

#[test]
fn missing_histogram_edges_do_not_get_masked_by_other_format_unknown() -> Result<(), String> {
    let fixture = Fixture::with_wait([25, 75], Duration::from_secs(2))?;
    fixture
        .producer
        .required_data_mode
        .store(1, Ordering::Relaxed);
    fixture
        .producer
        .histogram_bins_absent
        .store(true, Ordering::Relaxed);
    fixture
        .producer
        .hierarchy_unknown
        .store(true, Ordering::Relaxed);
    fixture
        .release
        .send(())
        .map_err(|error| error.to_string())?;
    let view = fixture.open()?;
    let response = fixture.ok("viz.render", histogram_body(&view, 1))?;
    assert_eq!(response["kind"], "unavailable");
    assert_eq!(response["reasonKey"], "viz.unavailable.no_item_sizes");
    assert_eq!(response["availableFormats"], json!([]));
    Ok(())
}

#[test]
fn unavailable_scope_reference_fails_before_runtime_job_or_row_publication() -> Result<(), String> {
    let fixture = Fixture::with_wait([25, 75], Duration::ZERO)?;
    let view = fixture.open()?;
    let mut body = histogram_body(&view, 1);
    body["aggregateScopeRef"]["itemNo"] = json!(999);
    body["navigationScopeRef"]["itemNo"] = json!(999);
    let error = fixture
        .call("viz.render", body)
        .error
        .ok_or("invalid row source was accepted")?;
    assert_eq!(error.reason_key, "ref.unavailable");
    assert!(fixture.events.try_recv().is_err());
    Ok(())
}

#[test]
fn child_evidence_failure_fails_job_without_fallback_or_model_publication() -> Result<(), String> {
    let fixture = Fixture::with_wait([25, 75], Duration::ZERO)?;
    fixture
        .producer
        .required_data_mode
        .store(1, Ordering::Relaxed);
    fixture
        .producer
        .histogram_bins_absent
        .store(true, Ordering::Relaxed);
    fixture.producer.child_fails.store(true, Ordering::Relaxed);
    let view = fixture.open()?;
    let pending = fixture.render(&view, 1)?;
    let physical = job(&pending)?;
    fixture.finished(&physical, JobState::Failed)?;
    let missing = fixture.call(
        "viz.rows",
        json!({
            "modelRef": "fixture.unpublished",
            "resultRevision": id(3),
            "start": 0,
            "count": 10
        }),
    );
    assert_eq!(
        missing
            .error
            .ok_or("failed evidence published rows")?
            .reason_key,
        "viz.model_expired"
    );
    Ok(())
}
