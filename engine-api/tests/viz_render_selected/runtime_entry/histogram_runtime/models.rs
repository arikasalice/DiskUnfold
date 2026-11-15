use super::*;

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
fn lower_bound_in_open_last_bin_keeps_a_registered_model() -> Result<(), String> {
    let sink = IngestSink::default();
    let lower_bound = SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::LowerBound {
            bytes: 1_000_000_000_000_000_u64.into(),
        },
    };
    publish_histogram_values(&sink, &[Some(measured(75)), Some(lower_bound)])?;
    let fixture = Fixture::from_sink(sink, Duration::ZERO, NonZeroUsize::MIN.saturating_add(3))?;
    fixture
        .producer
        .required_data_mode
        .store(1, Ordering::Relaxed);

    let response = render_registered_histogram(&fixture)?;
    assert_eq!(response["kind"], "model", "{response}");
    assert_eq!(response["model"]["summary"]["unknownItems"]["value"], "0");
    assert_eq!(response["model"]["summary"]["groupedItems"]["value"], "2");
    for field in ["scope", "drawn", "grouped", "filteredOut", "remainder"] {
        assert_eq!(
            response["model"]["totals"][field]["quantity"]["unit"], "bytes",
            "total {field}"
        );
    }
    Ok(())
}

#[test]
fn maximum_size_is_in_the_open_last_bin_of_the_registered_histogram() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_histogram_values(&sink, &[Some(measured(u64::MAX))])?;
    let fixture = Fixture::from_sink(sink, Duration::ZERO, NonZeroUsize::MIN.saturating_add(3))?;
    fixture
        .producer
        .required_data_mode
        .store(1, Ordering::Relaxed);

    let response = render_registered_histogram(&fixture)?;
    assert_eq!(response["kind"], "model");
    assert_eq!(response["model"]["summary"]["unknownItems"]["value"], "0");
    assert_eq!(response["model"]["summary"]["groupedItems"]["value"], "1");
    let model_ref = response["model"]["modelRef"].as_str().ok_or("model ref")?;
    let rows = fixture.ok(
        "viz.rows",
        json!({
            "modelRef": model_ref,
            "resultRevision": id(3),
            "start": 0,
            "count": 10
        }),
    )?;
    assert_eq!(rows["totalCount"], 6);
    assert_eq!(rows["rows"][5]["quantity"]["value"], u64::MAX.to_string());
    assert_eq!(rows["rows"][5]["quantity"]["unit"], "bytes");
    Ok(())
}
