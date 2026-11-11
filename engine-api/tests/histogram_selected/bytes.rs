use super::*;
use du_core_contracts::visualization::quantity::VizAmount;

#[test]
fn byte_rows_preserve_lower_bounds_unknown_and_f03_denominator() -> Result<(), String> {
    check_case(false, false)?;
    check_case(true, false)?;
    check_case(false, true)
}
fn check_case(unknown: bool, mixed: bool) -> Result<(), String> {
    let sink = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/mixed"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_forest(id(1), vec![root])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    let amounts = if unknown {
        [None, None, None]
    } else if mixed {
        [
            Some(SizeAmount::Exact {
                bytes: 5_u64.into(),
            }),
            None,
            Some(SizeAmount::LowerBound {
                bytes: 1000_u64.pow(5).into(),
            }),
        ]
    } else {
        [
            Some(SizeAmount::Exact {
                bytes: 5_u64.into(),
            }),
            Some(SizeAmount::LowerBound {
                bytes: 1000_u64.pow(5).into(),
            }),
            Some(SizeAmount::Exact {
                bytes: 2000_u64.into(),
            }),
        ]
    };
    for (i, amount) in amounts.into_iter().enumerate() {
        build
            .add_file_at(
                0,
                Observation {
                    path: vec![format!("{i}").into_bytes()],
                    size: amount.map(|amount| SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount,
                    }),
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let scope = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let d = dispatcher()?;
    let selected = PinnedRendered::compute(
        version.clone(),
        histogram_dispatch::Request {
            version: &version,
            scope: &scope,
            size_basis: SizeBasis::Logical,
            unit_table: &SI_UNITS,
            unit_table_revision: UNIT_TABLE_REVISION,
            bounds: bounds(),
            basis: Basis::Bytes,
            scale: Scale::Automatic,
            unknown_reason: "fixture.unknown",
        },
        &d,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let bins = selected.rendered().prepared().unit_bins().bins.clone();
    let members: Vec<_> = selected
        .rendered()
        .prepared()
        .model()
        .histogram
        .bins
        .iter()
        .flat_map(|bin| bin.members.iter().copied())
        .collect();
    let candidate = histogram::prepare_selected(
        histogram::Request {
            version: version.clone(),
            scope: &scope,
            size_basis: SizeBasis::Logical,
            bins: &bins,
            bounds: bounds(),
            scale: Scale::Automatic,
            fingerprint: "mixed".into(),
            unknown_reason: "fixture.unknown".into(),
            bin_presentation: (0..bins.len())
                .map(|i| BinPresentation {
                    group_ref: format!("g{i}"),
                    display_name: format!("bin {i}"),
                    legend_class_key: "c".into(),
                    display_flags: vec![],
                })
                .collect(),
            item_presentation: members
                .into_iter()
                .map(|n| RowPresentation {
                    item: ItemRef {
                        result_version: id(2),
                        item_no: n,
                    },
                    legend_class_key: "c".into(),
                    display_flags: vec![],
                })
                .collect(),
        },
        selected,
        &mut || false,
    );
    let candidate = candidate.map_err(|e| format!("{e:?}"))?;
    if mixed {
        assert!(candidate.no_attribution());
        assert!(
            candidate
                .model()
                .histogram
                .bins
                .iter()
                .all(|bin| bin.rect.is_none())
        );
        let pages = ModelPages::new(sink, NonZeroUsize::MIN, Arc::new(Ids(AtomicU8::new(10))));
        let mut pending = pages
            .defer_histogram_pages(candidate, &mut || false)
            .map_err(|e| format!("{e:?}"))?;
        assert!(pending.no_attribution());
        let reference = pages
            .commit_histogram_pages(&mut pending, &mut || false)
            .map_err(|e| format!("{e:?}"))?;
        assert!(pages.histogram_frame(&reference).is_err());
        let rows = pages
            .rows(&VizRowsRequest {
                model_ref: reference,
                result_revision: id(2),
                start: 0,
                count: 10,
            })
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(rows.rows.len(), bins.len());
        return Ok(());
    }
    let rows = &candidate.pages().rows;
    assert_eq!(rows.len(), bins.len());
    assert!(
        rows.iter()
            .all(|r| r.quantity.as_ref().is_some_and(|q| q.unit() == "bytes"))
    );
    if unknown {
        assert!(rows.iter().all(|r| matches!(
            r.quantity.as_ref().map(|q| q.amount()),
            Some(VizAmount::Unknown { .. })
        )));
    } else {
        assert!(matches!(
            rows[0].quantity.as_ref().ok_or("q")?.amount(),
            VizAmount::Exact { .. }
        ));
        assert!(matches!(
            rows.last()
                .ok_or("last")?
                .quantity
                .as_ref()
                .ok_or("q")?
                .amount(),
            VizAmount::LowerBound { .. }
        ));
    }
    assert_eq!(
        rows[0].denominator.size.as_ref(),
        Some(&candidate.model().summary.aggregate.logical)
    );
    assert_eq!(rows[0].denominator.value.unit(), "bytes");
    assert_eq!(
        candidate.pages().groups["g0"].rows.len(),
        usize::from(!unknown)
    );
    assert_eq!(
        candidate.pages().groups["g5"].rows.len(),
        usize::from(!unknown)
    );
    let pages = ModelPages::new(
        sink,
        NonZeroUsize::MIN.saturating_add(3),
        Arc::new(Ids(AtomicU8::new(10))),
    );
    let mut pending = pages
        .defer_histogram_pages(candidate, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let reference = pages
        .commit_histogram_pages(&mut pending, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        pages
            .histogram_frame(&reference)
            .map_err(|e| format!("{e:?}"))?
            .axis_max,
        None
    );
    let published = pages
        .rows(&VizRowsRequest {
            model_ref: reference,
            result_revision: id(2),
            start: 0,
            count: 2,
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        published.rows[0].quantity.as_ref().ok_or("q")?.unit(),
        "bytes"
    );
    Ok(())
}
