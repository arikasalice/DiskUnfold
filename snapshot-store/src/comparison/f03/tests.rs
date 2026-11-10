use super::*;
use crate::{
    comparison::{RootComparison, folder::Child, range, summary, tree},
    records::item::{Item, Kind, Size},
    storage_match::{Capability, Observation as StorageObservation},
};
use du_analysis_index::{FileIdentity, IngestSink, Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    Id, SizeAmount, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead},
};

fn id(n: u64) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn value(bytes: u64, basis: SizeBasis) -> SizeValue {
    SizeValue {
        basis,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
        evidence: SizeEvidence::OsReported,
    }
}
fn version(n: u64, bytes: u64, complete: ResultCompleteness) -> Result<Arc<ResultVersion>, String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(n)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    if n == 1 {
        build
            .add_directory(vec![b"0".to_vec()])
            .map_err(|e| format!("{e:?}"))?;
    }
    for name in [b"a", b"b"] {
        build
            .add_with_space(
                Observation {
                    path: vec![name.to_vec()],
                    size: Some(value(bytes, SizeBasis::Logical)),
                },
                SpaceObservation {
                    allocated: Some(value(bytes, SizeBasis::Allocated)),
                    identity: Some(FileIdentity {
                        volume: b"v".to_vec(),
                        file: b"shared".to_vec(),
                    }),
                    link_count: Some(2),
                    sharing: SharingEvidence::None,
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(n + 1)?, complete)
        .map_err(|e| format!("{e:?}"))
}
fn references(version: &ResultVersion) -> Vec<ItemRef> {
    version
        .items()
        .iter()
        .filter(|item| item.kind == ResultItemKind::File)
        .map(|item| ItemRef {
            result_version: version.version_id().clone(),
            item_no: item.item_no,
        })
        .collect()
}
fn root() -> RootComparison {
    let o = StorageObservation {
        capability: Capability::Supported,
        kind: Some(b"id"),
        value: Some(b"v"),
    };
    RootComparison::new(o, o)
}

fn record(number: u32, name: &'static [u8], bytes: u64) -> Item<'static> {
    Item {
        number,
        parent: 0,
        kind: Kind::File,
        flags: 0,
        name,
        sizes: [Size::Exact(bytes), Size::Exact(bytes), Size::Unknown],
        modified_ms: None,
        identity: &[],
        link_count: 0,
    }
}

#[test]
fn actual_f03_set_totals_deduplicate_and_compare_pinned_sides() -> Result<(), String> {
    let before = version(1, 10, ResultCompleteness::Complete)?;
    let after = version(3, 20, ResultCompleteness::Complete)?;
    let mut before_refs = references(&before);
    assert_ne!(before_refs[0].item_no, references(&after)[0].item_no);
    before_refs.push(before_refs[0].clone());
    let mut totals = Totals::new(
        Selection::new(before.clone(), before_refs, true),
        Selection::new(after.clone(), references(&after), true),
    );
    let a = record(1, b"a", 10);
    let b = record(2, b"b", 10);
    let a2 = record(7, b"a", 20);
    let b2 = record(8, b"b", 20);
    let earlier_tree = tree::Tree::new(
        &[
            Child {
                item: &a,
                key: b"a",
            },
            Child {
                item: &b,
                key: b"b",
            },
        ],
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let later_tree = tree::Tree::new(
        &[
            Child {
                item: &a2,
                key: b"a",
            },
            Child {
                item: &b2,
                key: b"b",
            },
        ],
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let ranges = range::Ranges::new(&[], || false).map_err(|e| format!("{e:?}"))?;
    let comparison = summary::compare(
        summary::Inputs {
            root: &root(),
            earlier: &earlier_tree,
            later: &later_tree,
            basis: &SizeBasis::Allocated,
            ranges: &ranges,
        },
        &mut totals,
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(comparison.rows.len(), 2);
    let totals = comparison.totals.ok_or("missing totals")?;
    assert_eq!(
        totals.earlier.allocated.amount,
        SizeAmount::Exact {
            bytes: 10_u64.into()
        }
    );
    assert_eq!(
        totals.later.allocated.amount,
        SizeAmount::Exact {
            bytes: 20_u64.into()
        }
    );
    assert_eq!(totals.delta, Some(10));
    Ok(())
}

#[test]
fn wrong_version_incomplete_membership_and_partial_sources_fail_closed() -> Result<(), String> {
    let before = version(11, 10, ResultCompleteness::Complete)?;
    let after = version(13, 20, ResultCompleteness::Partial)?;
    let mut wrong = Totals::new(
        Selection::new(before.clone(), references(&after), true),
        Selection::new(after.clone(), vec![], true),
    );
    assert!(matches!(
        wrong.total(Side::Earlier, &SizeBasis::Logical, &mut || false),
        Err(SpaceTotalError::Item(
            du_analysis_index::QueryError::StaleVersion
        ))
    ));
    let mut totals = Totals::new(
        Selection::new(before.clone(), references(&before), false),
        Selection::new(after.clone(), references(&after), true),
    );
    for side in [Side::Earlier, Side::Later] {
        let total = totals
            .total(side, &SizeBasis::Allocated, &mut || false)
            .map_err(|e| format!("{e:?}"))?;
        assert_ne!(total.allocated_completeness, ResultCompleteness::Complete);
        assert_eq!(total.allocated.amount, SizeAmount::Unknown);
    }
    assert!(matches!(
        totals.total(Side::Earlier, &SizeBasis::Allocated, &mut || true),
        Err(SpaceTotalError::Cancelled)
    ));
    Ok(())
}
