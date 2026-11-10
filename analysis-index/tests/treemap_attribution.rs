#![allow(clippy::unwrap_used)]

use du_analysis_index::{
    FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence, SpaceObservation,
    visualization::{
        squarify::Rect,
        treemap::{self, AdmissionError, ChildKey},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};
use std::sync::Arc;

fn id(n: u8) -> Id {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).unwrap()
}
fn size(basis: SizeBasis, n: u64) -> SizeValue {
    SizeValue {
        evidence: if basis == SizeBasis::Logical {
            SizeEvidence::Measured
        } else {
            SizeEvidence::OsReported
        },
        basis,
        amount: SizeAmount::Exact { bytes: n.into() },
    }
}
fn obs(volume: u8, file: u8, count: Option<u64>) -> SpaceObservation {
    SpaceObservation {
        allocated: Some(size(SizeBasis::Allocated, 10)),
        identity: Some(FileIdentity {
            volume: vec![volume],
            file: vec![file],
        }),
        link_count: count,
        sharing: SharingEvidence::None,
    }
}
fn build(
    a: Option<SpaceObservation>,
    b: Option<SpaceObservation>,
    completeness: ResultCompleteness,
) -> Arc<ResultVersion> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).unwrap();
    for p in [vec![], vec![b"a".to_vec()], vec![b"b".to_vec()]] {
        build.add_directory(p).unwrap();
    }
    for (name, physical) in [(b"a", a), (b"b", b)] {
        let file = Observation {
            path: vec![name.to_vec(), b"file".to_vec()],
            size: Some(size(SizeBasis::Logical, 10)),
        };
        if let Some(physical) = physical {
            build.add_with_space(file, physical).unwrap();
        } else {
            build.add_file(file).unwrap();
        }
    }
    build.seal(id(2), completeness).unwrap()
}
fn layout(v: &ResultVersion, basis: SizeBasis) -> Result<treemap::Partition, AdmissionError> {
    let root = ItemRef {
        result_version: v.version_id().clone(),
        item_no: 0,
    };
    let keys: Vec<_> = v
        .items()
        .iter()
        .filter(|item| item.parent == Some(0))
        .map(|item| ChildKey {
            item: ItemRef {
                result_version: v.version_id().clone(),
                item_no: item.item_no,
            },
            path_compare_key: item.name.clone(),
        })
        .collect();
    treemap::layout_parent(
        v,
        &root,
        basis,
        Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        &keys,
    )
}
#[test]
fn same_identity_across_siblings_rejects_physical_but_logical_is_per_path() {
    let v = build(
        Some(obs(1, 7, Some(2))),
        Some(obs(1, 7, Some(2))),
        ResultCompleteness::Complete,
    );
    for basis in [SizeBasis::Allocated, SizeBasis::UniquePhysical] {
        assert_eq!(
            layout(&v, basis).unwrap_err(),
            AdmissionError::NoAttribution
        );
    }
    let partition = layout(&v, SizeBasis::Logical).unwrap();
    assert_eq!(partition.layout.shapes.len(), 2);
    assert_eq!(partition.children.len(), 2);
    assert_eq!(
        v.space(0).unwrap().allocated.amount,
        size(SizeBasis::Allocated, 10).amount
    );
}
#[test]
fn independent_identities_and_different_volumes_are_admitted() {
    for (volume, file) in [(1, 8), (2, 7)] {
        let v = build(
            Some(obs(1, 7, Some(1))),
            Some(obs(volume, file, Some(1))),
            ResultCompleteness::Complete,
        );
        for basis in [
            SizeBasis::Allocated,
            SizeBasis::UniquePhysical,
            SizeBasis::Logical,
        ] {
            assert_eq!(layout(&v, basis).unwrap().layout.shapes.len(), 2);
        }
    }
}
#[test]
fn uncertain_physical_observations_and_partial_physical_totals_are_rejected() {
    let mut no_identity = obs(1, 8, Some(1));
    no_identity.identity = None;
    let mut no_bytes = obs(1, 8, Some(1));
    no_bytes.allocated = None;
    let mut shared = obs(1, 8, Some(1));
    shared.sharing = SharingEvidence::Unknown;
    for other in [
        None,
        Some(no_identity),
        Some(no_bytes),
        Some(shared),
        Some(obs(1, 8, None)),
        Some(obs(1, 8, Some(0))),
    ] {
        let v = build(
            Some(obs(1, 7, Some(1))),
            other,
            ResultCompleteness::Complete,
        );
        assert_eq!(
            layout(&v, SizeBasis::Allocated).unwrap_err(),
            AdmissionError::NoAttribution
        );
        assert!(layout(&v, SizeBasis::Logical).is_ok());
    }
    let v = build(
        Some(obs(1, 7, Some(1))),
        Some(obs(1, 8, Some(1))),
        ResultCompleteness::Partial,
    );
    assert_eq!(
        layout(&v, SizeBasis::Allocated).unwrap_err(),
        AdmissionError::NoAttribution
    );
    assert_eq!(
        layout(&v, SizeBasis::Logical).unwrap().layout.shapes.len(),
        2
    );
}

#[test]
fn links_inside_one_sibling_are_deduplicated_and_counts_are_checked() {
    for links in [1, 2] {
        let sink = IngestSink::default();
        let mut b = sink.open(id(1)).unwrap();
        b.add_directory(vec![]).unwrap();
        b.add_directory(vec![b"a".to_vec()]).unwrap();
        for name in [b"one", b"two"] {
            b.add_with_space(
                Observation {
                    path: vec![b"a".to_vec(), name.to_vec()],
                    size: Some(size(SizeBasis::Logical, 10)),
                },
                obs(1, 7, Some(links)),
            )
            .unwrap();
        }
        let v = b.seal(id(2), ResultCompleteness::Complete).unwrap();
        assert_eq!(layout(&v, SizeBasis::Allocated).is_ok(), links == 2);
        assert_eq!(
            layout(&v, SizeBasis::Logical).unwrap().layout.shapes.len(),
            1
        );
    }
}

#[test]
fn recorded_gap_rejects_layout() {
    use du_core_contracts::{
        PathBytes, PathEncoding,
        result::{ResultNativePath, ResultScopeRoot},
    };
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let sink = IngestSink::default();
    let mut b = sink.open_forest(id(1), vec![root]).unwrap();
    b.add_directory_at(0, vec![]).unwrap();
    b.record_gap_at(0, vec![], "scan.path_denied".into(), None)
        .unwrap();
    let v = b.seal(id(2), ResultCompleteness::Complete).unwrap();
    assert!(!v.scope_gaps().is_empty());
    assert_eq!(
        layout(&v, SizeBasis::Logical).unwrap_err(),
        AdmissionError::NoAttribution
    );
}

#[test]
fn unrelated_recorded_gap_does_not_hide_complete_nested_partition() {
    use du_core_contracts::{
        PathBytes, PathEncoding,
        result::{ResultNativePath, ResultScopeRoot},
    };
    for gap in [
        b"outside".as_slice(),
        b"selected".as_slice(),
        b"".as_slice(),
    ] {
        let root = ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/root"),
            },
            volume_id: vec![1],
            file_id: vec![1],
        };
        let sink = IngestSink::default();
        let mut b = sink.open_observed(id(1), root).unwrap();
        for name in [b"selected".as_slice(), b"outside".as_slice()] {
            b.add_directory(vec![name.to_vec()]).unwrap();
        }
        b.add_directory(vec![]).unwrap();
        for (name, file) in [(b"first".as_slice(), 1), (b"second".as_slice(), 2)] {
            b.add_with_space(
                Observation {
                    path: vec![b"selected".to_vec(), name.to_vec()],
                    size: Some(size(SizeBasis::Logical, 10)),
                },
                obs(1, file, Some(1)),
            )
            .unwrap();
        }
        b.record_gap_at(
            0,
            if gap.is_empty() {
                vec![]
            } else {
                vec![gap.to_vec(), b"unread".to_vec()]
            },
            "scan.path_denied".into(),
            None,
        )
        .unwrap();
        let v = b.seal(id(2), ResultCompleteness::Complete).unwrap();
        assert_eq!(v.completeness(), ResultCompleteness::Missing);
        let selected = v
            .items()
            .iter()
            .find(|item| item.name == b"selected")
            .unwrap();
        let parent = ItemRef {
            result_version: v.version_id().clone(),
            item_no: selected.item_no,
        };
        let keys: Vec<_> = v
            .items()
            .iter()
            .filter(|item| item.parent == Some(selected.item_no))
            .map(|item| ChildKey {
                item: ItemRef {
                    result_version: v.version_id().clone(),
                    item_no: item.item_no,
                },
                path_compare_key: item.name.clone(),
            })
            .collect();
        let bounds = Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        for basis in [SizeBasis::Logical, SizeBasis::Allocated] {
            let result = treemap::layout_parent(&v, &parent, basis, bounds, &keys);
            if gap == b"outside" {
                assert_eq!(result.unwrap().layout.shapes.len(), 2);
            } else {
                assert_eq!(result.unwrap_err(), AdmissionError::NoAttribution);
            }
        }
    }
}

#[test]
fn nested_unprovable_partition_suppresses_children_without_redistributing() {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)).unwrap();
    for path in [
        vec![],
        vec![b"container".to_vec()],
        vec![b"container".to_vec(), b"a".to_vec()],
        vec![b"container".to_vec(), b"b".to_vec()],
    ] {
        b.add_directory(path).unwrap();
    }
    for name in [b"a", b"b"] {
        b.add_with_space(
            Observation {
                path: vec![b"container".to_vec(), name.to_vec(), b"file".to_vec()],
                size: Some(size(SizeBasis::Logical, 10)),
            },
            obs(1, 7, Some(2)),
        )
        .unwrap();
    }
    let v = b.seal(id(2), ResultCompleteness::Complete).unwrap();
    assert_eq!(
        layout(&v, SizeBasis::Allocated)
            .unwrap()
            .layout
            .shapes
            .len(),
        1
    );
    let container = v
        .items()
        .iter()
        .find(|item| item.name == b"container")
        .unwrap();
    let parent = ItemRef {
        result_version: v.version_id().clone(),
        item_no: container.item_no,
    };
    let keys: Vec<_> = v
        .items()
        .iter()
        .filter(|item| item.parent == Some(container.item_no))
        .map(|item| ChildKey {
            item: ItemRef {
                result_version: v.version_id().clone(),
                item_no: item.item_no,
            },
            path_compare_key: item.name.clone(),
        })
        .collect();
    let bounds = Rect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
    };
    assert_eq!(
        treemap::layout_parent(&v, &parent, SizeBasis::Allocated, bounds, &keys).unwrap_err(),
        AdmissionError::NoAttribution
    );
    let branch =
        treemap::layout_nested_parent(&v, &parent, SizeBasis::Allocated, bounds, &keys).unwrap();
    assert!(branch.layout.shapes.is_empty());
    assert_eq!(branch.children.len(), 2);
    assert!(branch.children.iter().all(|child| !child.area_attributed
        && child.value.amount == size(SizeBasis::Allocated, 10).amount));
    let mut bad_keys = keys.clone();
    bad_keys.pop();
    assert_eq!(
        treemap::layout_nested_parent(&v, &parent, SizeBasis::Allocated, bounds, &bad_keys)
            .unwrap_err(),
        AdmissionError::InvalidKeys
    );
}
