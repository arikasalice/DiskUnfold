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

#[test]
fn runtime_publishes_byte_histogram_and_fixed_rows() -> Result<(), String> {
    let fixture = Fixture::new([25, 75])?;
    let first = exercise(&fixture, false)?;
    fixture.producer.unit_iec.store(true, Ordering::SeqCst);
    let view = fixture.open()?;
    let second = render(&fixture, &view, 1, false)?;
    let second = if second["kind"] == "job" {
        fixture.finished(&job(&second)?, JobState::Done)?;
        render(&fixture, &view, 2, false)?
    } else {
        second
    };
    let reference = second["model"]["modelRef"].as_str().ok_or("second model")?;
    assert_ne!(first["model"]["modelRef"], second["model"]["modelRef"]);
    assert_ne!(
        first["model"]["conditionFingerprint"],
        second["model"]["conditionFingerprint"]
    );
    assert_eq!(
        fixture
            .session
            .histogram_frame(reference)
            .map_err(|e| format!("{e:?}"))?
            .unit_bins
            .base,
        1024
    );
    Ok(())
}

#[test]
fn runtime_publishes_hardlink_fallback_and_fixed_rows() -> Result<(), String> {
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
    let mut build = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for name in [b"a", b"b"] {
        build
            .add_with_space_at(
                0,
                Observation {
                    path: vec![name.to_vec()],
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact {
                            bytes: 5_u64.into(),
                        },
                    }),
                },
                SpaceObservation {
                    allocated: Some(SizeValue {
                        basis: SizeBasis::Allocated,
                        evidence: SizeEvidence::OsReported,
                        amount: SizeAmount::Exact {
                            bytes: 10_u64.into(),
                        },
                    }),
                    identity: Some(FileIdentity {
                        volume: vec![1],
                        file: vec![2],
                    }),
                    link_count: Some(2),
                    sharing: SharingEvidence::None,
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(3), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let fixture = Fixture::from_sink(sink, Duration::ZERO, NonZeroUsize::MIN.saturating_add(3))?;
    exercise(&fixture, true)?;
    Ok(())
}

#[test]
fn histogram_cancel_and_source_replacement_prevent_publication() -> Result<(), String> {
    for replacement in [false, true] {
        let fixture = Fixture::new([25, 75])?;
        let view = fixture.open()?;
        let physical = job(&render(&fixture, &view, 1, false)?)?;
        fixture
            .entered
            .recv_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        if replacement {
            publish(&fixture.sink, id(2), id(4))?;
        } else {
            assert_eq!(fixture.detach(&view, 1)?["controlResult"], "accepted");
        }
        fixture.release.send(()).map_err(|e| e.to_string())?;
        let counts = finished_counts(
            &fixture,
            &physical,
            if replacement {
                JobState::Failed
            } else {
                JobState::Cancelled
            },
        )?;
        assert_eq!(counts.completed, 0);
        assert_eq!(counts.unprocessed, if replacement { 0 } else { 2 });
        if replacement {
            let mut body = render_body(&view, 2);
            body["format"] = json!("histogram");
            assert_eq!(
                fixture
                    .call("viz.render", body)
                    .error
                    .ok_or("stale source accepted")?
                    .reason_key,
                "result.version_stale"
            );
        } else {
            let next = render(&fixture, &view, 2, false)?;
            if next["kind"] == "job" {
                let next_job = job(&next)?;
                assert_ne!(physical, next_job);
                fixture.finished(&next_job, JobState::Done)?;
                assert_eq!(render(&fixture, &view, 3, false)?["kind"], "model");
            } else {
                assert_eq!(next["kind"], "model");
            }
        }
    }
    Ok(())
}

#[test]
fn histogram_unit_is_frozen_before_worker_runs() -> Result<(), String> {
    let fixture = Fixture::new([25, 75])?;
    let view = fixture.open()?;
    let physical = job(&render(&fixture, &view, 1, false)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    fixture.producer.unit_iec.store(true, Ordering::SeqCst);
    fixture.release.send(()).map_err(|e| e.to_string())?;
    fixture.finished(&physical, JobState::Done)?;
    let original = fixture
        .session
        .histogram_frame(&{
            // The already committed reference belongs to the work admitted before the setting changed.
            // Read it from the finished work by repeating the original unit selection.
            fixture.producer.unit_iec.store(false, Ordering::SeqCst);
            render(&fixture, &view, 2, false)?["model"]["modelRef"]
                .as_str()
                .ok_or("model")?
                .to_owned()
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(original.unit_bins.base, 1000);
    fixture.producer.unit_iec.store(true, Ordering::SeqCst);
    let response = render(&fixture, &view, 3, false)?;
    let response = if response["kind"] == "job" {
        fixture.finished(&job(&response)?, JobState::Done)?;
        render(&fixture, &view, 4, false)?
    } else {
        response
    };
    let reference = response["model"]["modelRef"].as_str().ok_or("model")?;
    assert_eq!(
        fixture
            .session
            .histogram_frame(reference)
            .map_err(|e| format!("{e:?}"))?
            .unit_bins
            .base,
        1024
    );
    Ok(())
}

#[test]
fn completed_count_includes_unknown_observations_and_excludes_bins() -> Result<(), String> {
    for empty in [false, true] {
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
        let mut build = sink
            .open_forest_in_lineage(id(2), vec![root], lineage)
            .map_err(|e| format!("{e:?}"))?;
        build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        if !empty {
            for (name, size) in [
                (
                    b'a',
                    Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact {
                            bytes: 25_u64.into(),
                        },
                    }),
                ),
                (b'b', None),
            ] {
                build
                    .add_file(Observation {
                        path: vec![vec![name]],
                        size,
                    })
                    .map_err(|e| format!("{e:?}"))?;
            }
        }
        build
            .seal(id(3), ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        let fixture =
            Fixture::from_sink(sink, Duration::ZERO, NonZeroUsize::MIN.saturating_add(3))?;
        let view = fixture.open()?;
        let response = render(&fixture, &view, 1, false)?;
        let physical = if response["kind"] == "job" {
            Some(job(&response)?)
        } else {
            None
        };
        if !empty {
            fixture
                .entered
                .recv_timeout(Duration::from_secs(3))
                .map_err(|e| e.to_string())?;
            fixture.release.send(()).map_err(|e| e.to_string())?;
        }
        let counts = if let Some(physical) = physical {
            finished_counts(&fixture, &physical, JobState::Done)?
        } else {
            assert_eq!(response["kind"], "model");
            loop {
                let event = fixture
                    .events
                    .recv_timeout(Duration::from_secs(3))
                    .map_err(|e| e.to_string())?;
                if let JobEvent::Finished { summary, .. } = event {
                    assert_eq!(summary.state, JobState::Done);
                    break summary.counts.clone();
                }
            }
        };
        assert_eq!(counts.completed, if empty { 0 } else { 2 });
        assert_eq!(counts.failed + counts.skipped + counts.unprocessed, 0);
    }
    Ok(())
}
