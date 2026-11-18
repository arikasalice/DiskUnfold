use super::*;

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
