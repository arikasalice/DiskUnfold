use super::*;

#[test]
fn unknown_histogram_item_returns_a_table_when_other_items_have_sizes() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_histogram_values(&sink, &[Some(measured(25)), None])?;
    let fixture = Fixture::from_sink(sink, Duration::ZERO, NonZeroUsize::MIN.saturating_add(3))?;
    fixture
        .producer
        .required_data_mode
        .store(1, Ordering::Relaxed);

    let response = render_registered_histogram(&fixture)?;
    assert_fallback_table(&fixture, &response)
}

#[test]
fn ambiguous_lower_bound_histogram_item_returns_a_table() -> Result<(), String> {
    let sink = IngestSink::default();
    let lower_bound = SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::LowerBound {
            bytes: 25_u64.into(),
        },
    };
    publish_histogram_values(&sink, &[Some(measured(75)), Some(lower_bound)])?;
    let fixture = Fixture::from_sink(sink, Duration::ZERO, NonZeroUsize::MIN.saturating_add(3))?;
    fixture
        .producer
        .required_data_mode
        .store(1, Ordering::Relaxed);

    let response = render_registered_histogram(&fixture)?;
    assert_fallback_table(&fixture, &response)
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
