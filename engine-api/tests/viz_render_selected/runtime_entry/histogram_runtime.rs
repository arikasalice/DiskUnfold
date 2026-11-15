use super::*;
use du_analysis_index::{FileIdentity, SharingEvidence, SpaceObservation};
use du_core_contracts::size_units::UNIT_TABLE_REVISION;

pub(super) fn facts(producer: &Producer) -> du_engine_api::viz_runtime::HistogramFacts {
    du_engine_api::viz_runtime::HistogramFacts {
        unit_table: if producer.unit_iec.load(Ordering::SeqCst) {
            &du_core_contracts::size_units::IEC_UNITS
        } else {
            &du_core_contracts::size_units::SI_UNITS
        },
        unit_table_revision: UNIT_TABLE_REVISION,
        bins: (0..6)
            .map(|i| du_engine_api::viz_pages::histogram::BinPresentation {
                group_ref: format!("g{i}"),
                display_name: format!("bin {i}"),
                legend_class_key: "fixture.class".into(),
                display_flags: vec![],
            })
            .collect(),
        unknown_reason: "fixture.unknown".into(),
    }
}

fn publish_histogram_values(sink: &IngestSink, values: &[Option<SizeValue>]) -> Result<(), String> {
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
    let mut build = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    for (index, size) in values.iter().enumerate() {
        build
            .add_file(Observation {
                path: vec![format!("f{index}").into_bytes()],
                size: size.clone(),
            })
            .map_err(|error| format!("{error:?}"))?;
    }
    build
        .seal(id(3), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

fn render_registered_histogram(fixture: &Fixture) -> Result<Value, String> {
    let view = fixture.open()?;
    let first = render(fixture, &view, 1, false)?;
    if first["kind"] != "job" {
        return Ok(first);
    }
    let physical = job(&first)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|error| error.to_string())?;
    fixture
        .release
        .send(())
        .map_err(|error| error.to_string())?;
    fixture.finished(&physical, JobState::Done)?;
    render(fixture, &view, 2, false)
}

fn assert_fallback_table(fixture: &Fixture, response: &Value) -> Result<(), String> {
    assert_eq!(response["kind"], "unavailable", "{response}");
    assert_eq!(response["reasonKey"], "viz.unavailable.no_attribution");
    let fallback = &response["fallback"];
    let model_ref = fallback["modelRef"].as_str().ok_or("fallback model ref")?;
    let frame = fixture
        .session
        .histogram_frame(model_ref)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        frame.status,
        du_engine_api::viz_pages::histogram::HistogramStatus::NoAttribution
    );
    let rows = fixture.ok(
        "viz.rows",
        json!({
            "modelRef": model_ref,
            "resultRevision": id(3),
            "start": 0,
            "count": 10
        }),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_eq!(rows["totalCount"], 6);
    Ok(())
}

fn measured(bytes: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}
pub(super) fn dispatcher()
-> Result<du_analysis_index::visualization::typed_layout::Dispatcher, String> {
    let mut registry = du_analysis_index::visualization::registry::Builder::<
        Box<dyn du_analysis_index::visualization::typed_layout::Strategy>,
    >::default();
    bundle::register_implemented(&mut registry).map_err(|e| format!("{e:?}"))?;
    bundle::register_histogram_candidate(&mut registry).map_err(|e| format!("{e:?}"))?;
    Ok(
        du_analysis_index::visualization::typed_layout::Dispatcher::new(
            registry.build(&[]).map_err(|e| format!("{e:?}"))?,
        ),
    )
}

fn render(fixture: &Fixture, view: &Value, seq: u32, allocated: bool) -> Result<Value, String> {
    let mut body = render_body(view, seq);
    body["format"] = json!("histogram");
    if allocated {
        body["sizeBasis"] = json!("allocated");
    }
    fixture.ok("viz.render", body)
}
fn finished_counts(
    fixture: &Fixture,
    job: &Id,
    state: JobState,
) -> Result<du_core_contracts::job::JobCounts, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while let Ok(event) = fixture
        .events
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        if let JobEvent::Finished {
            summary,
            history_recorded,
            ..
        } = event
            && &summary.job_id == job
        {
            assert_eq!(summary.state, state);
            assert!(!history_recorded);
            return Ok(summary.counts.clone());
        }
    }
    Err("terminal F24 event missing".into())
}
fn exercise(fixture: &Fixture, allocated: bool) -> Result<Value, String> {
    let view = fixture.open()?;
    let first = job(&render(fixture, &view, 1, allocated)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let counts = finished_counts(fixture, &first, JobState::Done)?;
    assert_eq!(counts.completed, 2);
    assert_eq!(counts.failed + counts.skipped + counts.unprocessed, 0);
    let result = render(fixture, &view, 2, allocated)?;
    let reference = if allocated {
        &result["fallback"]["modelRef"]
    } else {
        &result["model"]["modelRef"]
    };
    let model_ref = reference.as_str().ok_or("model reference missing")?;
    let frame = fixture
        .session
        .histogram_frame(model_ref)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(frame.unit_bins.table_revision, UNIT_TABLE_REVISION);
    assert_eq!(frame.unit_bins.base, 1000);
    assert_eq!(
        frame.basis,
        du_analysis_index::visualization::histogram::Basis::Bytes
    );
    let rows = fixture.ok(
        "viz.rows",
        json!({"modelRef":reference,"resultRevision":id(3),"start":0,"count":10}),
    )?;
    assert_eq!(rows["totalCount"], 6);
    assert_eq!(rows["resultRevision"], json!(id(3)));
    if allocated {
        assert_eq!(result["kind"], "unavailable");
        assert_eq!(result["reasonKey"], "viz.unavailable.no_attribution");
        assert_eq!(
            frame.status.reason_key(),
            Some("viz.unavailable.no_attribution")
        );
        assert!(frame.bins.iter().all(|bin| bin.rect.is_none()));
        assert_eq!(rows["rows"][0]["quantity"]["value"], "10");
    } else {
        assert_eq!(result["kind"], "model");
        assert_eq!(
            frame.status,
            du_engine_api::viz_pages::histogram::HistogramStatus::Ready
        );
        assert!(
            !result["model"]["shapes"]
                .as_array()
                .ok_or("shapes")?
                .is_empty()
        );
        assert_eq!(rows["rows"][0]["quantity"]["value"], "100");
    }
    Ok(result)
}

mod fallbacks;
mod lifecycle;
mod models;
