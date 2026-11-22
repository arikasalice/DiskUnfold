use super::*;
use crate::{IngestSink, Observation};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use std::sync::{Arc, Barrier};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn fixture() -> Result<Arc<ResultVersion>, String> {
    fixture_with_limit(None)
}
fn fixture_with_limit(limit: Option<usize>) -> Result<Arc<ResultVersion>, String> {
    fixture_ordered(limit, [b"a", b"b", b"c"])
}
fn fixture_ordered(limit: Option<usize>, names: [&[u8]; 3]) -> Result<Arc<ResultVersion>, String> {
    fixture_ordered_with_count(limit, names, Some(3))
}
fn fixture_ordered_with_count(
    limit: Option<usize>,
    names: [&[u8]; 3],
    link_count: Option<u64>,
) -> Result<Arc<ResultVersion>, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root])
        .map_err(|e| format!("{e:?}"))?;
    if let Some(limit) = limit {
        build
            .set_shared_table_limit(limit)
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    assert!(build.set_shared_table_limit(10).is_err());
    for name in names {
        build
            .add_with_space_at(
                0,
                Observation {
                    path: vec![name.to_vec()],
                    size: None,
                },
                SpaceObservation {
                    allocated: Some(SizeValue {
                        basis: SizeBasis::Allocated,
                        evidence: SizeEvidence::OsReported,
                        amount: SizeAmount::Exact { bytes: 8.into() },
                    }),
                    identity: Some(FileIdentity {
                        volume: vec![1],
                        file: vec![2],
                    }),
                    link_count,
                    sharing: SharingEvidence::Unknown,
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))
}

fn fixture_single(
    identity: Option<FileIdentity>,
    link_count: Option<u64>,
) -> Result<Arc<ResultVersion>, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_with_space_at(
            0,
            Observation {
                path: vec![b"a".to_vec()],
                size: None,
            },
            SpaceObservation {
                allocated: Some(SizeValue {
                    basis: SizeBasis::Allocated,
                    evidence: SizeEvidence::OsReported,
                    amount: SizeAmount::Exact { bytes: 8.into() },
                }),
                identity,
                link_count,
                sharing: SharingEvidence::Unknown,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn the_counted_path_does_not_depend_on_enumeration_order() -> Result<(), String> {
    let mut counted = Vec::new();
    let orders: [[&[u8]; 3]; 3] = [[b"a", b"b", b"c"], [b"c", b"b", b"a"], [b"b", b"c", b"a"]];
    for names in orders {
        let result = fixture_ordered(None, names)?;
        let reference = ItemRef {
            result_version: id(2)?,
            item_no: 1,
        };
        let links = result
            .space_detail(&reference)
            .map_err(|e| format!("{e:?}"))?
            .links
            .ok_or("missing links")?;
        let item = links.counted.ok_or("no counted path")?;
        let (_, parts) = result
            .relative_components(&item)
            .map_err(|e| format!("{e:?}"))?;
        counted.push(parts.concat());
    }
    assert!(counted.iter().all(|name| name == b"a"));
    Ok(())
}

#[test]
fn sharing_requires_in_scope_identity_paths_not_a_link_count() -> Result<(), String> {
    let unidentified = fixture_single(None, Some(2))?;
    let item = ItemRef {
        result_version: id(2)?,
        item_no: 1,
    };
    let links = unidentified
        .space_detail(&item)
        .map_err(|e| format!("{e:?}"))?
        .links
        .ok_or("missing links")?;
    assert!(links.counted.is_none());
    assert!(
        unidentified
            .shared_counted_item(&item)
            .map_err(|e| format!("{e:?}"))?
            .is_none()
    );

    let unknown_count = fixture_single(
        Some(FileIdentity {
            volume: vec![1],
            file: vec![2],
        }),
        None,
    )?;
    let single = unknown_count
        .space_detail(&item)
        .map_err(|e| format!("{e:?}"))?
        .links
        .ok_or("missing links")?;
    assert_eq!(single.observed_link_count, None);
    assert!(single.counted.is_none());

    let unknown_count = fixture_ordered_with_count(None, [b"a", b"b", b"c"], None)?;
    let reference = ItemRef {
        result_version: id(2)?,
        item_no: 1,
    };
    let links = unknown_count
        .space_detail(&reference)
        .map_err(|e| format!("{e:?}"))?
        .links
        .ok_or("missing links")?;
    assert_eq!(links.observed_link_count, None);
    assert_eq!(links.counted.as_ref().map(|item| item.item_no), Some(1));
    Ok(())
}
#[test]
fn the_counted_path_compares_components_by_bytes_without_folding() -> Result<(), String> {
    let cases: [([&[u8]; 3], &[u8]); 2] = [
        // A shorter name that is a prefix of another comes first.
        ([b"ab", b"a", b"b"], b"a"),
        // Byte order: no case folding or normalization, so "B" < "a".
        ([b"a", b"B", b"b"], b"B"),
    ];
    for (names, expected) in cases {
        let result = fixture_ordered(None, names)?;
        let reference = ItemRef {
            result_version: id(2)?,
            item_no: 1,
        };
        let links = result
            .space_detail(&reference)
            .map_err(|e| format!("{e:?}"))?
            .links
            .ok_or("missing links")?;
        let item = links.counted.ok_or("no counted path")?;
        let (_, parts) = result
            .relative_components(&item)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(parts.concat(), expected);
    }
    Ok(())
}
#[test]
fn frozen_limit_controls_lazy_lookup_for_each_result() -> Result<(), String> {
    let earlier = fixture_with_limit(Some(3))?;
    let later = fixture_with_limit(Some(2))?;
    let reference = ItemRef {
        result_version: id(2)?,
        item_no: 1,
    };
    let later_links = later
        .space_detail(&reference)
        .map_err(|e| format!("{e:?}"))?
        .links
        .ok_or("missing links")?;
    assert_eq!(
        later_links.lookup_unavailable_reason,
        Some("space.dedup_capacity")
    );
    let earlier_links = earlier
        .space_detail(&reference)
        .map_err(|e| format!("{e:?}"))?
        .links
        .ok_or("missing links")?;
    assert_eq!(earlier_links.in_scope.ok_or("missing group")?.len(), 3);
    let zero = fixture_with_limit(Some(0))?;
    assert_eq!(
        zero.space(0).ok_or("missing root")?.allocated.amount,
        SizeAmount::Unknown
    );
    assert!(
        zero.space(0)
            .ok_or("missing root")?
            .limitations
            .contains(&"space.dedup_capacity")
    );
    assert_eq!(
        zero.space_detail(&reference)
            .map_err(|e| format!("{e:?}"))?
            .links
            .ok_or("missing links")?
            .lookup_unavailable_reason,
        Some("space.dedup_capacity")
    );
    Ok(())
}
#[test]
fn candidate_entries_are_bounded_even_for_one_identity() -> Result<(), String> {
    let result = fixture()?;
    assert!(build_groups(&result, 2, false).is_err());
    let groups = build_groups(&result, 3, false).map_err(|_| "unexpected limit")?;
    assert_eq!(groups.0.len(), 1);
    assert_eq!(
        groups
            .0
            .values()
            .next()
            .ok_or("missing group")?
            .members
            .len(),
        3
    );
    Ok(())
}
#[test]
fn concurrent_readers_initialize_once_and_release_with_version() -> Result<(), String> {
    let result = fixture()?;
    let weak = Arc::downgrade(&result);
    let barrier = Arc::new(Barrier::new(8));
    let mut workers = Vec::new();
    for _ in 0..8 {
        let result = Arc::clone(&result);
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            let groups = result
                .identity_groups
                .get_or_init(|| build_groups(&result, 3, false));
            groups
                .as_ref()
                .map(|value| std::ptr::from_ref(value) as usize)
                .ok()
        }));
    }
    let pointers: Vec<_> = workers
        .into_iter()
        .map(|w| w.join().map_err(|_| "worker panic"))
        .collect::<Result<_, _>>()?;
    assert!(pointers[0].is_some());
    assert!(pointers.iter().all(|pointer| *pointer == pointers[0]));
    drop(result);
    assert!(weak.upgrade().is_none());
    Ok(())
}

#[test]
fn exhausted_table_returns_unknown_without_partial_group() -> Result<(), String> {
    let result = fixture()?;
    result
        .identity_groups
        .set(Err(()))
        .map_err(|_| "already initialized")?;
    let reference = ItemRef {
        result_version: id(2)?,
        item_no: 1,
    };
    let detail = result
        .space_detail(&reference)
        .map_err(|e| format!("{e:?}"))?;
    let links = detail.links.ok_or("missing links")?;
    assert_eq!(
        links.lookup_unavailable_reason,
        Some("space.dedup_capacity")
    );
    assert!(links.in_scope.is_none());
    assert_eq!(
        detail.exclusive,
        ExclusiveDecision::Unknown {
            reason_key: "space.dedup_capacity"
        }
    );
    assert_eq!(detail.shared, SharedSpace::Unknown);
    Ok(())
}
