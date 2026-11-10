//! Exercise the reusable read-only whole-scan entry without filesystem access.
use du_analysis_index::{FileIdentity, IngestSink, Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    ByteSize, Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    Timestamp,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use du_engine_api::duplicate_scope::{Error, select_whole_scan};

fn id(n: u8) -> Id {
    Id::from_v7_bytes([1, 2, 3, 4, 5, n, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, n])
}
fn root(file: Vec<u8>) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
        },
        volume_id: vec![1],
        file_id: file,
    }
}
fn build(
    observed: bool,
    file_id: Vec<u8>,
    completeness: ResultCompleteness,
    gap: bool,
) -> Result<IngestSink, String> {
    let source = IngestSink::default();
    let mut build = if observed {
        source.open_observed(id(1), root(file_id))
    } else {
        source.open(id(1))
    }
    .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![b"directory".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let path = vec![b"directory".to_vec(), b"file".to_vec()];
    build
        .add_with_space(
            Observation {
                path: path.clone(),
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::OsReported,
                    amount: SizeAmount::Exact {
                        bytes: ByteSize::from(8),
                    },
                }),
            },
            SpaceObservation {
                allocated: None,
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![2],
                }),
                link_count: None,
                sharing: SharingEvidence::Unknown,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_modified_at(0, &path, Timestamp::parse("2026-01-01T00:00:00.000Z")?)
        .map_err(|e| format!("{e:?}"))?;
    if gap {
        build
            .record_gap_at(
                0,
                vec![b"unknown".to_vec()],
                "scan.path_denied".into(),
                None,
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(2), completeness)
        .map_err(|e| format!("{e:?}"))?;
    Ok(source)
}

#[test]
fn selects_only_the_pinned_scan_root_without_user_scope_refs() -> Result<(), String> {
    let source = build(true, vec![1], ResultCompleteness::Complete, false)?;
    let selected = select_whole_scan(&source, &id(2), 8, || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(selected.version(), &id(2));
    assert_eq!(selected.files().len(), 1);
    assert_eq!(selected.files()[0].size, 8);
    assert_eq!(selected.files()[0].item.result_version, id(2));
    assert!(selected.selected(&selected.files()[0].item).is_some());
    let (root, components) = selected
        .anchored_location(&selected.files()[0].item)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(root.file_id, [1]);
    assert_eq!(components, [b"directory".as_slice(), b"file".as_slice()]);
    assert_eq!(
        selected.anchored_location(&ItemRef {
            result_version: id(2),
            item_no: 0
        }),
        Err(Error::InvalidScope)
    );
    let empty = select_whole_scan(&source, &id(2), 9, || false).map_err(|e| format!("{e:?}"))?;
    assert!(empty.files().is_empty());
    assert_eq!(empty.counts().below_minimum, 1);
    Ok(())
}

#[test]
fn missing_partial_and_unread_scan_evidence_fail_closed() -> Result<(), String> {
    for source in [
        build(false, vec![1], ResultCompleteness::Complete, false)?,
        build(true, vec![], ResultCompleteness::Complete, false)?,
        build(true, vec![1], ResultCompleteness::Partial, false)?,
        build(true, vec![1], ResultCompleteness::Complete, true)?,
    ] {
        assert!(matches!(
            select_whole_scan(&source, &id(2), 0, || false),
            Err(Error::Unavailable)
        ));
    }
    assert!(matches!(
        select_whole_scan(&IngestSink::default(), &id(2), 0, || false),
        Err(Error::MissingVersion)
    ));
    Ok(())
}

#[test]
fn stale_and_cancelled_requests_return_no_selection() -> Result<(), String> {
    let source = build(true, vec![1], ResultCompleteness::Complete, false)?;
    assert!(matches!(
        select_whole_scan(&source, &id(3), 0, || false),
        Err(Error::StaleVersion)
    ));
    assert!(matches!(
        select_whole_scan(&source, &id(2), 0, || true),
        Err(Error::Cancelled)
    ));
    let mut checks = 0;
    assert!(matches!(
        select_whole_scan(&source, &id(2), 0, || {
            checks += 1;
            checks == 4
        }),
        Err(Error::Cancelled)
    ));
    Ok(())
}

#[test]
fn multi_root_results_are_not_silently_treated_as_single_root() -> Result<(), String> {
    let source = IngestSink::default();
    let mut build = source
        .open_forest(id(1), vec![root(vec![1]), root(vec![2])])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(1, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        select_whole_scan(&source, &id(2), 0, || false),
        Err(Error::Unavailable)
    ));
    Ok(())
}

#[test]
fn publication_during_selection_does_not_switch_the_pinned_source() -> Result<(), String> {
    let source = build(true, vec![1], ResultCompleteness::Complete, false)?;
    let mut checks = 0;
    let mut publication = None;
    let selected = select_whole_scan(&source, &id(2), 0, || {
        checks += 1;
        if checks == 2 {
            publication = Some((|| -> Result<(), String> {
                let mut later = source.open(id(3)).map_err(|e| format!("{e:?}"))?;
                later.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
                later
                    .seal(id(4), ResultCompleteness::Complete)
                    .map_err(|e| format!("{e:?}"))?;
                Ok(())
            })());
        }
        false
    })
    .map_err(|e| format!("{e:?}"))?;
    publication.ok_or("publication callback not reached")??;
    assert_eq!(selected.version(), &id(2));
    assert_eq!(selected.files().len(), 1);
    assert!(matches!(
        select_whole_scan(&source, &id(2), 0, || false),
        Err(Error::StaleVersion)
    ));
    Ok(())
}
