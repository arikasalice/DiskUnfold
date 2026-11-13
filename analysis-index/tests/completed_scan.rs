use du_analysis_index::{BuildHandle, CompletedScanError, IngestSink, QueryError, ScanCompletion};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}

fn scan(source: &IngestSink, scope: u8, lineage: &Id) -> Result<BuildHandle, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut build = source
        .open_forest_in_lineage(id(scope), vec![root], lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|error| format!("{error:?}"))?;
    Ok(build)
}

#[test]
fn delayed_terminal_evidence_follows_the_exact_baseline_through_rollback() -> Result<(), String> {
    let source = IngestSink::default();
    let lineage = source
        .bind_current_analysis("fixture", id(1))
        .map_err(|error| format!("{error:?}"))?;
    let first = scan(&source, 2, &lineage)?
        .seal(id(3), ResultCompleteness::Partial)
        .map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        source.pin_completed_scan(&id(3)),
        Err(CompletedScanError::Unconfirmed)
    ));

    let mut next = scan(&source, 4, &lineage)?;
    next.publish_partial(id(5))
        .map_err(|error| format!("{error:?}"))?;
    assert!(
        !source
            .observe_scan_terminal(&id(5), Some(ScanCompletion::Done))
            .map_err(|error| format!("{error:?}"))?
    );
    assert!(
        source
            .observe_scan_terminal(&id(3), Some(ScanCompletion::Partial))
            .map_err(|error| format!("{error:?}"))?
    );
    assert!(matches!(
        source.pin_completed_scan(&id(3)),
        Err(CompletedScanError::Lookup(QueryError::StaleVersion))
    ));
    drop(next);

    let admitted = source
        .pin_completed_scan(&id(3))
        .map_err(|error| format!("{error:?}"))?;
    assert!(std::sync::Arc::ptr_eq(admitted.version(), &first));
    assert_eq!(admitted.completion(), ScanCompletion::Partial);
    assert_eq!(
        admitted.version().completeness(),
        ResultCompleteness::Partial
    );
    scan(&source, 6, &lineage)?
        .seal(id(7), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(admitted.version().version_id(), &id(3));
    assert!(matches!(
        source.pin_completed_scan(&id(3)),
        Err(CompletedScanError::Lookup(QueryError::StaleVersion))
    ));
    Ok(())
}

#[test]
fn cancelled_or_unsuccessful_terminal_cannot_confirm_a_scan() -> Result<(), String> {
    let source = IngestSink::default();
    let lineage = source
        .bind_current_analysis("fixture", id(1))
        .map_err(|error| format!("{error:?}"))?;
    scan(&source, 2, &lineage)?
        .seal(id(3), ResultCompleteness::Cancelled)
        .map_err(|error| format!("{error:?}"))?;
    assert!(
        !source
            .observe_scan_terminal(&id(3), Some(ScanCompletion::Done))
            .map_err(|error| format!("{error:?}"))?
    );
    assert!(matches!(
        source.pin_completed_scan(&id(3)),
        Err(CompletedScanError::Unconfirmed)
    ));
    scan(&source, 4, &lineage)?
        .seal(id(5), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    assert!(
        !source
            .observe_scan_terminal(&id(5), None)
            .map_err(|error| format!("{error:?}"))?
    );
    assert!(
        !source
            .observe_scan_terminal(&id(5), Some(ScanCompletion::Done))
            .map_err(|error| format!("{error:?}"))?
    );
    assert!(matches!(
        source.pin_completed_scan(&id(5)),
        Err(CompletedScanError::Unconfirmed)
    ));
    Ok(())
}
