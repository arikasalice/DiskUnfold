use super::*;

fn folder() -> ResultItem {
    ResultItem {
        item_no: 0,
        parent: None,
        name: vec![],
        size: None,
        kind: du_core_contracts::result::ResultItemKind::Directory,
    }
}

fn file_value(bytes: u64) -> Known {
    Known {
        bytes,
        lower: false,
        sharing: SharingEvidence::None,
    }
}

/// #270: a file whose identity occurs once is no sharing candidate, so any number
/// of them stays exact under a small capacity.
#[test]
fn files_seen_once_do_not_reach_the_capacity() {
    let mut sum = Physical::with_limit(1);
    for bytes in [10, 20, 30] {
        sum.add_known(&file_value(bytes));
        sum.untabled.add(&file_value(bytes));
    }
    let mut aggregates =
        crate::space::aggregate(&[folder()], &[], ResultCompleteness::Complete, None);
    let root = aggregates.get_mut(0).expect("root aggregate");
    sum.finish(ResultCompleteness::Complete, root);
    let exact = SizeAmount::Exact {
        bytes: ByteSize::from(60),
    };
    assert_eq!(root.allocated.amount, exact);
    assert_eq!(root.unique_physical.amount, exact);
    assert!(!root.limitations.contains(&"space.dedup_capacity"));
}

/// #270: over the capacity both totals are the lower bound of the files seen once;
/// the candidates the table kept are left out, as they depend on the order.
#[test]
fn over_the_capacity_both_totals_are_the_lower_bound_of_files_seen_once() {
    let mut sum = Physical::with_limit(1);
    sum.add_known(&file_value(7));
    sum.untabled.add(&file_value(7));
    for file in 1..=2 {
        sum.insert(
            FileIdentity {
                volume: vec![1],
                file: vec![file],
            },
            Some(file_value(100)),
        );
    }
    let mut aggregates =
        crate::space::aggregate(&[folder()], &[], ResultCompleteness::Complete, None);
    let root = aggregates.get_mut(0).expect("root aggregate");
    sum.finish(ResultCompleteness::Complete, root);
    let lower = SizeAmount::LowerBound {
        bytes: ByteSize::from(7),
    };
    assert_eq!(root.allocated.amount, lower);
    assert_eq!(root.unique_physical.amount, lower);
    assert_eq!(root.allocated.evidence, SizeEvidence::OsReported);
    assert!(root.limitations.contains(&"space.dedup_capacity"));
}

#[test]
fn configured_capacity_deduplicates_and_rejects_new_identities() {
    let mut sum = Physical::with_limit(1);
    let identity = |n| FileIdentity {
        volume: vec![1],
        file: vec![n],
    };
    let known = Some(Known {
        bytes: 10,
        lower: false,
        sharing: SharingEvidence::None,
    });
    sum.insert(identity(1), known.clone());
    sum.insert(identity(1), known.clone());
    assert_eq!(sum.total, 10);
    assert!(!sum.capacity_exceeded);
    sum.insert(identity(2), known);
    assert!(sum.capacity_exceeded);
    assert_eq!(sum.identities.len(), 1);
    let mut empty = Physical::with_limit(0);
    empty.insert(identity(1), None);
    assert!(empty.capacity_exceeded);
}

#[test]
fn capacity_alone_makes_both_physical_totals_unknown() {
    let mut sum = Physical::with_limit(1);
    for file in 1..=2 {
        sum.insert(
            FileIdentity {
                volume: vec![1],
                file: vec![file],
            },
            Some(Known {
                bytes: 10,
                lower: false,
                sharing: SharingEvidence::None,
            }),
        );
    }
    let item = ResultItem {
        item_no: 0,
        parent: None,
        name: vec![],
        size: None,
        kind: du_core_contracts::result::ResultItemKind::Directory,
    };
    let mut aggregates = crate::space::aggregate(&[item], &[], ResultCompleteness::Complete, None);
    let root = aggregates.get_mut(0).expect("root aggregate");
    sum.finish(ResultCompleteness::Complete, root);
    assert_eq!(root.allocated.amount, SizeAmount::Unknown);
    assert_eq!(root.unique_physical.amount, SizeAmount::Unknown);
    assert!(root.limitations.contains(&"space.dedup_capacity"));
}

#[test]
fn identity_capacity_boundary_and_conflict_update_summary() {
    let mut sum = Physical::default();
    let identity = |n| FileIdentity {
        volume: vec![1],
        file: vec![n],
    };
    let known = |bytes| {
        Some(Known {
            bytes,
            lower: false,
            sharing: SharingEvidence::None,
        })
    };
    sum.insert_with_limit(identity(1), known(10), 2);
    sum.insert_with_limit(identity(2), known(20), 2);
    assert_eq!(
        (sum.total, sum.known, sum.capacity_exceeded),
        (30, 2, false)
    );
    let item = ResultItem {
        item_no: 0,
        parent: None,
        name: vec![],
        size: None,
        kind: du_core_contracts::result::ResultItemKind::Directory,
    };
    let mut aggregates = crate::space::aggregate(&[item], &[], ResultCompleteness::Complete, None);
    let root = aggregates.get_mut(0).expect("root aggregate");
    sum.finish(ResultCompleteness::Complete, root);
    assert_eq!(
        root.allocated.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(30)
        }
    );
    sum.insert_with_limit(identity(1), known(11), 2);
    assert_eq!((sum.total, sum.known, sum.conflicts), (20, 1, 1));
    sum.insert_with_limit(identity(3), known(5), 2);
    assert!(sum.capacity_exceeded);
    assert_eq!(sum.identities.len(), 2);
    sum.finish(ResultCompleteness::Complete, root);
    assert_eq!(root.allocated.amount, SizeAmount::Unknown);
    assert!(root.limitations.contains(&"space.dedup_capacity"));
}

#[test]
fn merge_inserts_only_the_smaller_identity_map() {
    let mut small = Physical::default();
    let mut large = Physical::default();
    for n in 0u8..128 {
        large.insert(
            FileIdentity {
                volume: vec![1],
                file: vec![n],
            },
            Some(Known {
                bytes: 1,
                lower: false,
                sharing: SharingEvidence::None,
            }),
        );
    }
    small.insert(
        FileIdentity {
            volume: vec![2],
            file: vec![0],
        },
        Some(Known {
            bytes: 2,
            lower: false,
            sharing: SharingEvidence::None,
        }),
    );
    small.merge(large);
    assert_eq!(small.identities.len(), 129);
    assert_eq!(small.insertions, 129);
    assert_eq!(small.total, 130);
}
