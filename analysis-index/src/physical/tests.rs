use super::*;

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
    let mut aggregates = crate::space::aggregate(&[item], &[true], ResultCompleteness::Complete);
    sum.finish(ResultCompleteness::Complete, &mut aggregates[0]);
    assert_eq!(aggregates[0].allocated.amount, SizeAmount::Unknown);
    assert_eq!(aggregates[0].unique_physical.amount, SizeAmount::Unknown);
    assert!(aggregates[0].limitations.contains(&"space.dedup_capacity"));
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
    let mut aggregates = crate::space::aggregate(&[item], &[true], ResultCompleteness::Complete);
    sum.finish(ResultCompleteness::Complete, &mut aggregates[0]);
    assert_eq!(
        aggregates[0].allocated.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(30)
        }
    );
    sum.insert_with_limit(identity(1), known(11), 2);
    assert_eq!((sum.total, sum.known, sum.conflicts), (20, 1, 1));
    sum.insert_with_limit(identity(3), known(5), 2);
    assert!(sum.capacity_exceeded);
    assert_eq!(sum.identities.len(), 2);
    sum.finish(ResultCompleteness::Complete, &mut aggregates[0]);
    assert_eq!(aggregates[0].allocated.amount, SizeAmount::Unknown);
    assert!(aggregates[0].limitations.contains(&"space.dedup_capacity"));
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
