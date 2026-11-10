use du_analysis_index::{
    FileIdentity, IngestSink, Observation, QueryError, SharingEvidence, SpaceObservation,
    VolumeReport, VolumeReportPoint,
    volume_gap::{FactorValue, RemainderUnavailable, VolumeGapTarget},
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    Timestamp,
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    scan_observation::VolumeSpace,
};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn root(name: &str, volume: &[u8]) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(name.as_bytes()),
        },
        volume_id: volume.to_vec(),
        file_id: vec![9],
    }
}
fn size(bytes: u64, basis: SizeBasis) -> SizeValue {
    SizeValue {
        basis,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}
fn point(used: u64) -> VolumeReportPoint {
    VolumeReportPoint {
        observed_at: Timestamp::from_unix_millis(used as i64),
        value: Ok(VolumeSpace {
            total_bytes: 100,
            used_bytes: used,
            free_bytes: 100 - used,
        }),
    }
}
fn fixture() -> Result<std::sync::Arc<du_analysis_index::ResultVersion>, String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(
            id(1)?,
            vec![
                root("a", &[1]),
                root("b", &[1]),
                root("c", &[2]),
                root("d", &[]),
            ],
        )
        .map_err(|e| format!("{e:?}"))?;
    for index in 0..4 {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    for index in 0..3 {
        let vol = if index < 2 { 1 } else { 2 };
        build
            .add_with_space_at(
                index,
                Observation {
                    path: vec![b"item".to_vec()],
                    size: Some(size(10, SizeBasis::Logical)),
                },
                SpaceObservation {
                    allocated: Some(size(10, SizeBasis::Allocated)),
                    identity: Some(FileIdentity {
                        volume: vec![vol],
                        file: vec![1],
                    }),
                    link_count: Some(2),
                    sharing: SharingEvidence::None,
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .record_gap_at(2, vec![], "scan.path_denied".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    let failure = PortError {
        kind: PortErrorKind::Io,
        reason_key: "space.volume_report_unavailable".into(),
        os_code: Some(5),
    };
    build
        .set_volume_reports(vec![
            VolumeReport {
                volume_id: vec![1],
                start: Some(point(40)),
                end: Some(VolumeReportPoint {
                    observed_at: Timestamp::from_unix_millis(2),
                    value: Err(failure),
                }),
            },
            VolumeReport {
                volume_id: vec![2],
                start: Some(point(30)),
                end: Some(point(35)),
            },
        ])
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(2)?, ResultCompleteness::Missing)
        .map_err(|e| format!("{e:?}"))
}
#[test]
fn selected_volume_deduplicates_and_excludes_other_volume_missing() -> Result<(), String> {
    let version = fixture()?;
    let report = version
        .volume_gap(VolumeGapTarget::Volume(&[1]))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(report.allocated_completeness, ResultCompleteness::Complete);
    assert_eq!(
        report.unique_physical_completeness,
        ResultCompleteness::Complete
    );
    assert_eq!(
        report.allocated.amount,
        SizeAmount::Exact { bytes: 10.into() }
    );
    assert_eq!(
        report.unique_physical.amount,
        SizeAmount::Exact { bytes: 10.into() }
    );
    assert!(report.start.as_ref().is_some_and(|p| p.value.is_ok()));
    assert!(report.end.as_ref().is_some_and(|p| p.value.is_err()));
    assert_eq!(
        report.gap.remainder,
        Err(RemainderUnavailable::VolumeReportUnavailable)
    );
    assert_eq!(
        report.volume_report_unavailable_reason,
        Some("space.volume_report_unavailable")
    );
    assert!(
        report
            .gap
            .factors
            .iter()
            .all(|r| matches!(r.value, FactorValue::Unknown { .. }))
    );
    let other = version
        .volume_gap(VolumeGapTarget::Volume(&[2]))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        other.allocated.amount,
        SizeAmount::LowerBound { bytes: 10.into() }
    );
    assert_eq!(other.gap.remainder, Err(RemainderUnavailable::InexactInput));
    Ok(())
}
#[test]
fn root_binding_rejects_other_versions_nonroots_and_unknown_volume_selection() -> Result<(), String>
{
    let version = fixture()?;
    let actual = ItemRef {
        result_version: id(2)?,
        item_no: 0,
    };
    let root = version
        .volume_gap(VolumeGapTarget::Root(&actual))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        root.allocated.amount,
        SizeAmount::Exact { bytes: 10.into() }
    );
    let unknown = ItemRef {
        result_version: id(2)?,
        item_no: 3,
    };
    let root = version
        .volume_gap(VolumeGapTarget::Root(&unknown))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(root.volume_id, None);
    assert_eq!(
        root.gap.remainder,
        Err(RemainderUnavailable::VolumeReportUnavailable)
    );
    assert!(matches!(
        version.volume_gap(VolumeGapTarget::Volume(&[])),
        Err(QueryError::InvalidPath)
    ));
    assert!(matches!(
        version.volume_gap(VolumeGapTarget::Volume(&[7])),
        Err(QueryError::InvalidPath)
    ));
    let wrong = ItemRef {
        result_version: id(3)?,
        item_no: 0,
    };
    assert!(matches!(
        version.volume_gap(VolumeGapTarget::Root(&wrong)),
        Err(QueryError::StaleVersion)
    ));
    let nonroot = ItemRef {
        result_version: id(2)?,
        item_no: 4,
    };
    assert!(matches!(
        version.volume_gap(VolumeGapTarget::Root(&nonroot)),
        Err(QueryError::InvalidPath)
    ));
    Ok(())
}

#[test]
fn selected_root_does_not_accept_file_identity_from_another_volume() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root("a", &[1])])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_with_space_at(
            0,
            Observation {
                path: vec![b"x".to_vec()],
                size: Some(size(10, SizeBasis::Logical)),
            },
            SpaceObservation {
                allocated: Some(size(10, SizeBasis::Allocated)),
                identity: Some(FileIdentity {
                    volume: vec![2],
                    file: vec![1],
                }),
                link_count: Some(1),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let report = result
        .volume_gap(VolumeGapTarget::Volume(&[1]))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(report.allocated.amount, SizeAmount::Unknown);
    assert_eq!(report.unique_physical.amount, SizeAmount::Unknown);
    Ok(())
}

#[test]
fn unattributed_missing_result_cannot_be_promoted_to_exact() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root("a", &[1])])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_with_space_at(
            0,
            Observation {
                path: vec![b"item".to_vec()],
                size: Some(size(10, SizeBasis::Logical)),
            },
            SpaceObservation {
                allocated: Some(size(10, SizeBasis::Allocated)),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![1],
                }),
                link_count: Some(1),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Missing)
        .map_err(|e| format!("{e:?}"))?;
    let gap = result
        .volume_gap(VolumeGapTarget::Volume(&[1]))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        gap.allocated.amount,
        SizeAmount::LowerBound { bytes: 10.into() }
    );
    assert_eq!(
        gap.unique_physical.amount,
        SizeAmount::LowerBound { bytes: 10.into() }
    );
    assert_eq!(gap.allocated_completeness, ResultCompleteness::Missing);
    assert_eq!(
        gap.unique_physical_completeness,
        ResultCompleteness::Missing
    );
    Ok(())
}

#[test]
fn selected_basis_completeness_preserves_unknown_sharing() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root("a", &[1])])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_with_space_at(
            0,
            Observation {
                path: vec![b"item".to_vec()],
                size: Some(size(10, SizeBasis::Logical)),
            },
            SpaceObservation {
                allocated: Some(size(10, SizeBasis::Allocated)),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![1],
                }),
                link_count: Some(1),
                sharing: SharingEvidence::Unknown,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let gap = result
        .volume_gap(VolumeGapTarget::Volume(&[1]))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(gap.allocated.amount, SizeAmount::Exact { bytes: 10.into() });
    assert_eq!(gap.allocated_completeness, ResultCompleteness::Complete);
    assert_eq!(gap.unique_physical.amount, SizeAmount::Unknown);
    assert_eq!(
        gap.unique_physical_completeness,
        ResultCompleteness::Missing
    );
    Ok(())
}
