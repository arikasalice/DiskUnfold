use super::*;
use du_core_contracts::visualization::quantity::VizAmount;

#[test]
fn unavailable_geometry_keeps_the_original_partial_rows_and_denominator() -> Result<(), String> {
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
    scan.add_file(Observation {
        path: vec![b"observed".to_vec()],
        size: Some(SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount: SizeAmount::Exact { bytes: 25.into() },
        }),
    })
    .map_err(|error| format!("{error:?}"))?;
    scan.add_file(Observation {
        path: vec![b"unmeasured".to_vec()],
        size: None,
    })
    .map_err(|error| format!("{error:?}"))?;
    let pinned = scan
        .publish_partial(id(3))
        .map_err(|error| format!("{error:?}"))?;
    let parent = ItemRef {
        result_version: pinned.version_id().clone(),
        item_no: 0,
    };
    let session = Session::new(
        sink.clone(),
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
        bundle::implemented_dispatcher().map_err(|error| format!("{error:?}"))?,
    );
    let render = || match session.render_selected(
        request(&parent, 1_000_000),
        |_, item, _| Ok(policy(item)),
        &mut || false,
    ) {
        Err(Error::NoAttribution(fallback)) => Ok(fallback),
        Err(error) => Err(format!("expected the original fallback, got {error:?}")),
        Ok(_) => Err("partial values unexpectedly received area".into()),
    };
    let fallback = render()?;
    assert_eq!(fallback, render()?);
    let rows_request = VizRowsRequest {
        model_ref: fallback.model_ref,
        result_revision: fallback.result_revision,
        start: 0,
        count: 10,
    };
    let page = session
        .rows(&rows_request)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(page.result_revision, id(3));
    assert_eq!(page.completeness, ResultCompleteness::Partial);
    assert_eq!(page.total_count, 2);
    for (row, name) in page.rows.iter().zip(["observed", "unmeasured"]) {
        assert_eq!(row.display_name, name);
        assert_eq!(row.ratio, None);
        assert_eq!(
            row.denominator.size.as_ref(),
            pinned.space(0).map(|aggregate| &aggregate.logical)
        );
        assert!(matches!(
            row.denominator.value.amount(),
            VizAmount::LowerBound { value } if value.get() == 25
        ));
    }
    assert_eq!(
        page.rows[0].size.amount,
        SizeAmount::LowerBound { bytes: 25.into() }
    );
    assert_eq!(page.rows[1].size.amount, SizeAmount::Unknown);
    scan.add_file(Observation {
        path: vec![b"later".to_vec()],
        size: Some(SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount: SizeAmount::Exact { bytes: 100.into() },
        }),
    })
    .map_err(|error| format!("{error:?}"))?;
    let later = scan
        .publish_partial(id(4))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        session.rows(&rows_request),
        Err(viz_pages::Error::StaleVersion)
    );
    let later_parent = ItemRef {
        result_version: id(4),
        item_no: 0,
    };
    let later_fallback = match session.render_selected(
        request(&later_parent, 1_000_000),
        |_, item, _| Ok(policy(item)),
        &mut || false,
    ) {
        Err(Error::NoAttribution(fallback)) => fallback,
        _ => return Err("expected later partial fallback".into()),
    };
    let later_page = session
        .rows(&VizRowsRequest {
            model_ref: later_fallback.model_ref,
            result_revision: later_fallback.result_revision,
            start: 0,
            count: 10,
        })
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(later_page.result_revision, id(4));
    assert_eq!(later_page.total_count, 3);
    assert_eq!(later_page.completeness, ResultCompleteness::Partial);
    for row in &later_page.rows {
        assert_eq!(
            row.denominator.size.as_ref(),
            later.space(0).map(|a| &a.logical)
        );
        assert!(
            matches!(row.denominator.value.amount(), VizAmount::LowerBound { value } if value.get() == 125)
        );
    }
    Ok(())
}
