use du_analysis_index::{
    BuildHandle, FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence,
    SpaceObservation,
    group::{self, Decision, Error},
    query::{Basis, Boundary, Condition, NameEncoding, NameTest, Predicate, Registry},
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};
use std::sync::Arc;

fn id(number: u8) -> Id {
    Id::from_v7_bytes([number; 16])
}

fn size(basis: SizeBasis, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
        evidence: SizeEvidence::OsReported,
    }
}

fn add(
    build: &mut BuildHandle,
    parent: &[u8],
    name: &[u8],
    identity: u8,
    bytes: u64,
) -> Result<(), String> {
    build
        .add_with_space(
            Observation {
                path: vec![parent.to_vec(), name.to_vec()],
                size: Some(size(SizeBasis::Logical, bytes)),
            },
            SpaceObservation {
                allocated: Some(size(SizeBasis::Allocated, bytes)),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![identity],
                }),
                link_count: Some(2),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|e| format!("{e:?}"))
}

fn fixture() -> Result<(Arc<ResultVersion>, ItemRef), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    for path in [
        vec![],
        vec![b"scope".to_vec()],
        vec![b"outside".to_vec()],
        vec![b"empty".to_vec()],
    ] {
        build.add_directory(path).map_err(|e| format!("{e:?}"))?;
    }
    add(&mut build, b"scope", b"a-alias", 1, 8)?;
    add(&mut build, b"scope", b"b-alias", 1, 8)?;
    add(&mut build, b"scope", b"z-other", 2, 4)?;
    add(&mut build, b"outside", b"a-alias", 3, 64)?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let root = version
        .items()
        .iter()
        .find(|item| item.name == b"scope")
        .ok_or("scope")?;
    let reference = ItemRef {
        result_version: id(2),
        item_no: root.item_no,
    };
    Ok((version, reference))
}

fn rule(name: NameTest) -> Condition {
    Condition::Predicate(Predicate::Name(name, NameEncoding::Bytes))
}

#[test]
fn scoped_overlap_and_union_use_independent_f03_totals() -> Result<(), String> {
    let (version, root) = fixture()?;
    let rules = [
        rule(NameTest::EndsWith(b"alias".to_vec())),
        rule(NameTest::StartsWith(b"a-".to_vec())),
    ];
    let read = group::classify(version, &root, &Registry::baseline(), &rules, || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(read.root(), &root);
    let sets = read
        .categories(&[0, 1, 1], || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(sets.union.len(), 2);
    assert_eq!(sets.overlap, [read.rows()[0].item.clone()]);
    assert_eq!(sets.unclassified, [read.rows()[2].item.clone()]);
    let totals = read.totals(&[], || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(totals.union.logical, size(SizeBasis::Logical, 16));
    assert_eq!(totals.union.allocated, size(SizeBasis::Allocated, 8));
    assert_eq!(totals.overlap.allocated, size(SizeBasis::Allocated, 8));
    assert_eq!(totals.scope.allocated, size(SizeBasis::Allocated, 12));
    let one = read
        .categories(&[1], || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(one.union.len(), 1);
    assert!(one.overlap.is_empty());
    assert_eq!(one.unclassified, sets.unclassified);
    assert_eq!(
        read.categories(&[2], || false),
        Err(Error::InvalidSelection)
    );
    Ok(())
}

#[test]
fn unavailable_rules_and_uncertain_predicates_preserve_membership() -> Result<(), String> {
    let (version, root) = fixture()?;
    let rules = [
        rule(NameTest::StartsWith(b"a-".to_vec())),
        Condition::Predicate(Predicate::Registered {
            id: "pred.absent".into(),
            revision: 1,
            operand: vec![],
        }),
    ];
    let read = group::classify(
        version.clone(),
        &root,
        &Registry::baseline(),
        &rules,
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        read.rows()[0].decisions,
        [Decision::Match, Decision::RuleUnavailable]
    );
    assert_eq!(
        read.rows()[0].decisions[1].reason_key(),
        Some("group.rule_unevaluable")
    );
    let sets = read
        .categories(&[], || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (
            sets.union.len(),
            sets.unclassified.len(),
            sets.undecidable.len()
        ),
        (1, 0, 2)
    );
    let empty = group::classify(version, &root, &Registry::baseline(), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        empty
            .categories(&[], || false)
            .map_err(|e| format!("{e:?}"))?
            .unclassified
            .len(),
        3
    );
    assert!(matches!(read.totals(&[], || true), Err(Error::Cancelled)));
    Ok(())
}

#[test]
fn missing_size_is_unknown_and_cancellation_discards_classification() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let root = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let rules = [Condition::Predicate(Predicate::Size(
        Basis::Logical,
        Boundary::AtLeast(1),
    ))];
    let registry = Registry::baseline();
    let read = group::classify(version.clone(), &root, &registry, &rules, || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(read.rows()[0].decisions, [Decision::Unknown]);
    let totals = read.totals(&[], || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(totals.undecidable.logical.amount, SizeAmount::Unknown);
    let mut checks = 0;
    assert!(matches!(
        group::classify(version, &root, &registry, &rules, || {
            checks += 1;
            checks == 3
        }),
        Err(Error::Cancelled)
    ));
    Ok(())
}

#[test]
fn empty_folder_stays_empty_and_another_version_is_rejected() -> Result<(), String> {
    let (version, _) = fixture()?;
    let empty = version
        .items()
        .iter()
        .find(|item| item.name == b"empty")
        .ok_or("empty")?;
    let root = ItemRef {
        result_version: id(2),
        item_no: empty.item_no,
    };
    let registry = Registry::baseline();
    let read = group::classify(version.clone(), &root, &registry, &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(read.rows().is_empty());
    let totals = read.totals(&[], || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        totals.scope.logical,
        SizeValue {
            evidence: SizeEvidence::Measured,
            ..size(SizeBasis::Logical, 0)
        }
    );
    let stale = ItemRef {
        result_version: id(3),
        ..root
    };
    assert!(matches!(
        group::classify(version, &stale, &registry, &[], || false),
        Err(Error::Source(du_analysis_index::QueryError::StaleVersion))
    ));
    Ok(())
}

#[test]
fn reconciliation_uses_all_rules_and_counts_known_and_unattributed_reasons_per_file()
-> Result<(), String> {
    fn unknown(
        _: &du_analysis_index::query::Record<'_>,
        _: &Predicate,
    ) -> du_analysis_index::query::Truth {
        du_analysis_index::query::Truth::Unknown
    }
    let (version, root) = fixture()?;
    let mut registry = Registry::baseline();
    registry
        .register_predicate("pred.unknown", 1, unknown)
        .map_err(|e| format!("{e:?}"))?;
    let unavailable = Condition::Predicate(Predicate::Registered {
        id: "pred.absent".into(),
        revision: 1,
        operand: vec![],
    });
    let rules = [
        rule(NameTest::StartsWith(b"a-".to_vec())),
        rule(NameTest::StartsWith(b"b-".to_vec())),
        Condition::Predicate(Predicate::Registered {
            id: "pred.unknown".into(),
            revision: 1,
            operand: vec![],
        }),
        unavailable.clone(),
        unavailable,
    ];
    let read = group::classify(version, &root, &registry, &rules, || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        read.categories(&[0], || false)
            .map_err(|e| format!("{e:?}"))?
            .union
            .len(),
        1
    );
    let summary = read.reconcile(|| false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (
            summary.observed_files,
            summary.matched_files,
            summary.unclassified_files,
            summary.undecidable_files
        ),
        (3, 2, 0, 1)
    );
    assert!(summary.source_coverage_complete);
    assert_eq!(
        summary.known_reason_counts.get("group.rule_unevaluable"),
        Some(&1)
    );
    assert_eq!(summary.unattributed_files, 1);
    assert!(matches!(read.reconcile(|| true), Err(Error::Cancelled)));
    Ok(())
}

#[test]
fn reconciliation_identifies_partial_coverage_and_empty_scope() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let partial = build.publish_partial(id(2)).map_err(|e| format!("{e:?}"))?;
    let root = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let read = group::classify(partial, &root, &Registry::baseline(), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    let summary = read.reconcile(|| false).map_err(|e| format!("{e:?}"))?;
    assert_eq!((summary.observed_files, summary.unclassified_files), (1, 1));
    assert!(!summary.source_coverage_complete);
    assert!(summary.known_reason_counts.is_empty());
    assert_eq!(summary.unattributed_files, 0);
    let (version, _) = fixture()?;
    let empty = version
        .items()
        .iter()
        .find(|i| i.name == b"empty")
        .ok_or("empty")?;
    let root = ItemRef {
        result_version: id(2),
        item_no: empty.item_no,
    };
    let read = group::classify(version, &root, &Registry::baseline(), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    let summary = read.reconcile(|| false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (
            summary.observed_files,
            summary.matched_files,
            summary.unclassified_files,
            summary.undecidable_files
        ),
        (0, 0, 0, 0)
    );
    assert!(summary.source_coverage_complete);
    let sink = IngestSink::default();
    let mut build = sink.open(id(4)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_untraversed_link(vec![b"link".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(5), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let root = ItemRef {
        result_version: id(5),
        item_no: 0,
    };
    let read = group::classify(version, &root, &Registry::baseline(), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        !read
            .reconcile(|| false)
            .map_err(|e| format!("{e:?}"))?
            .source_coverage_complete
    );
    Ok(())
}
