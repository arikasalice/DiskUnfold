mod scope_evidence;

use super::*;
use du_analysis_index::{Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    ByteSize, PathBytes, PathEncoding, SizeValue,
    result::{ResultNativePath, ResultScopeRoot},
};

fn select(source: &IngestSink, scope: Scope<'_>) -> Result<Selection, Error> {
    super::select(source, scope, || false)
}

fn id(n: u8) -> Id {
    Id::from_v7_bytes([1, 2, 3, 4, 5, n, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, n])
}

fn fixture() -> Result<(IngestSink, Arc<ResultVersion>), String> {
    fixture_with_root_file(vec![1])
}

fn fixture_with_root_file(root_file: Vec<u8>) -> Result<(IngestSink, Arc<ResultVersion>), String> {
    let source = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
        },
        volume_id: vec![1],
        file_id: root_file,
    };
    let mut build = source
        .open_observed(id(1), root)
        .map_err(|e| format!("{e:?}"))?;
    for path in [vec![], vec![b"a".to_vec()], vec![b"ab".to_vec()]] {
        build.add_directory(path).map_err(|e| format!("{e:?}"))?;
    }
    let paths = [
        (vec![b"a".to_vec(), b"excluded".to_vec()], 8, true, true),
        (vec![b"ab".to_vec(), b"inside".to_vec()], 8, true, true),
        (vec![b"ab".to_vec(), b"small".to_vec()], 2, true, true),
        (vec![b"ab".to_vec(), b"no-id".to_vec()], 8, false, true),
        (vec![b"ab".to_vec(), b"no-time".to_vec()], 8, true, false),
    ];
    for (index, (path, size, has_id, has_time)) in paths.into_iter().enumerate() {
        let observation = Observation {
            path: path.clone(),
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::OsReported,
                amount: SizeAmount::Exact {
                    bytes: ByteSize::from(size),
                },
            }),
        };
        let physical = SpaceObservation {
            allocated: None,
            identity: has_id.then(|| FileIdentity {
                volume: vec![1],
                file: vec![index as u8 + 2],
            }),
            link_count: None,
            sharing: SharingEvidence::Unknown,
        };
        build
            .add_with_space(observation, physical)
            .map_err(|e| format!("{e:?}"))?;
        if has_time {
            build
                .set_modified_at(0, &path, Timestamp::parse("2026-01-01T00:00:00.000Z")?)
                .map_err(|e| format!("{e:?}"))?;
            if path.last().is_some_and(|name| name == b"inside") {
                build
                    .set_native_modified_at(
                        0,
                        &path,
                        du_core_contracts::scan_observation::NativeModifiedTime::WindowsFiletime(
                            std::num::NonZeroU64::new(123_456_789).ok_or("zero")?,
                        ),
                    )
                    .map_err(|e| format!("{e:?}"))?;
            }
        }
    }
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok((source, version))
}

fn reference(version: &ResultVersion, name: &[u8]) -> Result<ItemRef, String> {
    let item = version
        .items()
        .iter()
        .find(|item| item.name == name)
        .ok_or("missing fixture item")?;
    Ok(ItemRef {
        result_version: version.version_id().clone(),
        item_no: item.item_no,
    })
}

#[test]
fn source_version_and_reference_must_match_at_admission() -> Result<(), String> {
    let (source, version) = fixture()?;
    let root = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let mixed = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    assert!(matches!(
        select(
            &source,
            Scope {
                version: &id(2),
                roots: &[mixed],
                exclusions: &[],
                min_bytes: 0
            }
        ),
        Err(Error::InvalidScope)
    ));
    assert!(matches!(
        select(
            &source,
            Scope {
                version: &id(3),
                roots: &[root],
                exclusions: &[],
                min_bytes: 0
            }
        ),
        Err(Error::StaleVersion)
    ));
    assert!(matches!(
        select(
            &source,
            Scope {
                version: &id(2),
                roots: &[reference(&version, b"inside")?],
                exclusions: &[],
                min_bytes: 0
            }
        ),
        Err(Error::InvalidScope)
    ));
    Ok(())
}

#[test]
fn selected_source_remains_pinned_after_current_result_changes() -> Result<(), String> {
    let (source, _) = fixture()?;
    let root = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let selection = select(
        &source,
        Scope {
            version: &id(2),
            roots: &[root],
            exclusions: &[],
            min_bytes: 0,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut later = source.open(id(3)).map_err(|e| format!("{e:?}"))?;
    later.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    later
        .seal(id(4), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(selection.version(), &id(2));
    assert!(selection.files().iter().any(|file| {
        file.native_modified
            == std::num::NonZeroU64::new(123_456_789)
                .map(du_core_contracts::scan_observation::NativeModifiedTime::WindowsFiletime)
    }));
    assert!(
        selection
            .scope_evidence()
            .roots
            .iter()
            .all(|root| root.result_version == id(2))
    );
    assert_eq!(
        selection.scope_evidence().source_completeness,
        ResultCompleteness::Complete
    );
    assert!(
        selection
            .files
            .iter()
            .all(|file| file.item.result_version == id(2))
    );
    assert!(matches!(
        select(
            &source,
            Scope {
                version: &id(2),
                roots: &[ItemRef {
                    result_version: id(2),
                    item_no: 0
                }],
                exclusions: &[],
                min_bytes: 0
            }
        ),
        Err(Error::StaleVersion)
    ));
    Ok(())
}

#[test]
fn outside_and_missing_scope_evidence_do_not_become_selected() -> Result<(), String> {
    let (source, version) = fixture()?;
    let ab = reference(&version, b"ab")?;
    let selection = select(
        &source,
        Scope {
            version: &id(2),
            roots: &[ab],
            exclusions: &[],
            min_bytes: 0,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(selection.counts().outside, 1);
    assert_eq!(selection.counts().unavailable, 2);
    assert!(matches!(
        select(
            &source,
            Scope {
                version: &id(2),
                roots: &[],
                exclusions: &[],
                min_bytes: 0
            }
        ),
        Err(Error::InvalidScope)
    ));
    assert!(matches!(
        select(
            &IngestSink::default(),
            Scope {
                version: &id(2),
                roots: &[ItemRef {
                    result_version: id(2),
                    item_no: 0
                }],
                exclusions: &[],
                min_bytes: 0
            }
        ),
        Err(Error::MissingVersion)
    ));
    Ok(())
}

#[test]
fn only_exact_observed_logical_sizes_can_pass_the_threshold() {
    let value = |basis, evidence, amount| SizeValue {
        basis,
        evidence,
        amount,
    };
    let exact = SizeAmount::Exact {
        bytes: ByteSize::from(0),
    };
    assert_eq!(
        exact_size(Some(&value(
            SizeBasis::Logical,
            SizeEvidence::OsReported,
            exact.clone()
        ))),
        Some(0)
    );
    assert_eq!(
        exact_size(Some(&value(
            SizeBasis::Logical,
            SizeEvidence::Estimated,
            exact.clone()
        ))),
        None
    );
    assert_eq!(
        exact_size(Some(&value(
            SizeBasis::Allocated,
            SizeEvidence::OsReported,
            exact
        ))),
        None
    );
    assert_eq!(
        exact_size(Some(&value(
            SizeBasis::Logical,
            SizeEvidence::OsReported,
            SizeAmount::LowerBound {
                bytes: ByteSize::from(9)
            }
        ))),
        None
    );
    assert_eq!(exact_size(None), None);
}

#[test]
fn empty_selection_keeps_pinned_version_without_creating_a_job() -> Result<(), String> {
    let (source, _) = fixture()?;
    let roots = [ItemRef {
        result_version: id(2),
        item_no: 0,
    }];
    let selected = select(
        &source,
        Scope {
            version: &id(2),
            roots: &roots,
            exclusions: &[],
            min_bytes: 9,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(selected.files().is_empty());
    assert_eq!(selected.version(), &id(2));
    assert_eq!(selected.counts().below_minimum, 5);
    Ok(())
}

#[test]
fn missing_root_identity_never_selects_observations() -> Result<(), String> {
    let (source, _) = fixture_with_root_file(vec![])?;
    let roots = [ItemRef {
        result_version: id(2),
        item_no: 0,
    }];
    let selected = select(
        &source,
        Scope {
            version: &id(2),
            roots: &roots,
            exclusions: &[],
            min_bytes: 0,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(selected.files().is_empty());
    assert_eq!(selected.counts().unavailable, 5);
    Ok(())
}

#[test]
fn selected_lookup_handles_first_last_missing_and_foreign_versions() -> Result<(), String> {
    let (source, version) = fixture()?;
    let roots = [ItemRef {
        result_version: id(2),
        item_no: 0,
    }];
    let selection = select(
        &source,
        Scope {
            version: &id(2),
            roots: &roots,
            exclusions: &[],
            min_bytes: 0,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(
        selection
            .files()
            .windows(2)
            .all(|pair| pair[0].item.item_no < pair[1].item.item_no)
    );
    for selected in selection.files() {
        assert_eq!(selection.selected(&selected.item), Some(selected));
    }
    assert!(
        selection
            .selected(&reference(&version, b"no-id")?)
            .is_none()
    );
    assert!(
        selection
            .selected(&ItemRef {
                result_version: id(3),
                item_no: selection.files()[0].item.item_no
            })
            .is_none()
    );
    Ok(())
}

#[test]
fn deep_tree_selection_is_cancellable_without_returning_partial_results() -> Result<(), String> {
    let source = IngestSink::default();
    let mut build = source.open(id(1)).map_err(|error| format!("{error:?}"))?;
    let mut path = Vec::new();
    build
        .add_directory(path.clone())
        .map_err(|error| format!("{error:?}"))?;
    for _ in 0..128 {
        path.push(b"d".to_vec());
        build
            .add_directory(path.clone())
            .map_err(|error| format!("{error:?}"))?;
    }
    path.push(b"leaf".to_vec());
    build
        .add_file(Observation { path, size: None })
        .map_err(|error| format!("{error:?}"))?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let roots = [ItemRef {
        result_version: id(2),
        item_no: 0,
    }];
    let scope = || Scope {
        version: version.version_id(),
        roots: &roots,
        exclusions: &[],
        min_bytes: 0,
    };
    assert!(matches!(
        super::select(&source, scope(), || true),
        Err(Error::Cancelled)
    ));
    let mut checks = 0;
    assert!(matches!(
        super::select(&source, scope(), || {
            checks += 1;
            checks == 64
        }),
        Err(Error::Cancelled)
    ));
    assert_eq!(checks, 64);
    let complete =
        super::select(&source, scope(), || false).map_err(|error| format!("{error:?}"))?;
    assert!(complete.files().is_empty());
    assert_eq!(complete.counts().unavailable, 1);
    Ok(())
}
