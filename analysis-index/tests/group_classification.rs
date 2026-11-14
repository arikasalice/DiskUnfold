mod group_support;

use std::sync::Arc;

use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    group::{self, Decision, Error, GroupCounts, Parallelism},
    query::{Basis, Boundary, Condition, Predicate},
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};
use group_support::{
    Recording, absent, add_file, alias_fixture as fixture, bytes, classify, ends_with, err,
    folder_ref, id, item_ref, registered, registry, size, starts_with,
};

fn default() -> Parallelism {
    Parallelism::default()
}

#[test]
fn overlap_and_unclassified_counts_and_independent_totals() -> Result<(), String> {
    let (version, scope) = fixture()?;
    let rules = vec![ends_with("alias"), starts_with("a-")];
    let read = classify(&version, Some(&scope), rules, default())?;
    assert_eq!(read.root(), Some(&scope));
    let summary = read.summary();
    assert_eq!(
        (
            summary.scope_files,
            summary.matched,
            summary.overlap,
            summary.unclassified,
            summary.undecidable
        ),
        (3, 2, 1, 1, 0)
    );
    assert!(summary.reconciled);
    assert_eq!(
        summary.groups,
        [
            GroupCounts {
                members: 2,
                overlap_members: 1,
                undecidable: 0
            },
            GroupCounts {
                members: 1,
                overlap_members: 1,
                undecidable: 0
            },
        ]
    );
    assert!(summary.reason_counts.is_empty());
    let totals = read.totals(&[], || false).map_err(err)?;
    assert_eq!(totals.union.logical, size(SizeBasis::Logical, 16));
    assert_eq!(totals.union.allocated, size(SizeBasis::Allocated, 8));
    assert_eq!(totals.overlap.allocated, size(SizeBasis::Allocated, 8));
    assert_eq!(totals.overlap_count, 1);
    assert_eq!(totals.scope.allocated, size(SizeBasis::Allocated, 12));
    assert_eq!(totals.unclassified.logical, size(SizeBasis::Logical, 4));
    assert_eq!(totals.groups.len(), 2);
    let second_only = read.totals(&[1, 1], || false).map_err(err)?;
    assert_eq!(second_only.union.logical, size(SizeBasis::Logical, 8));
    assert_eq!(second_only.overlap_count, 0);
    assert_eq!(second_only.unclassified, totals.unclassified);
    assert_eq!(second_only.groups, totals.groups);
    assert_eq!(
        read.totals(&[2], || false).map(|_| ()),
        Err(Error::InvalidSelection)
    );
    Ok(())
}

#[test]
fn an_empty_scope_is_a_confirmed_result_with_no_files() -> Result<(), String> {
    let (version, _) = fixture()?;
    let empty = folder_ref(&version, 0, "empty")?;
    assert_eq!(group::scope_file_count(&version, Some(&empty)), Ok(0));
    let read = classify(&version, Some(&empty), vec![starts_with("a")], default())?;
    let summary = read.summary();
    assert_eq!(
        (
            summary.scope_files,
            summary.matched,
            summary.overlap,
            summary.unclassified
        ),
        (0, 0, 0, 0)
    );
    assert!(summary.reconciled && summary.reason_counts.is_empty());
    assert_eq!(summary.groups, [GroupCounts::default()]);
    assert_eq!(read.rows().count(), 0);
    let totals = read.totals(&[], || false).map_err(err)?;
    let zero = |basis| SizeValue {
        evidence: SizeEvidence::Measured,
        ..size(basis, 0)
    };
    assert_eq!(totals.scope.logical, zero(SizeBasis::Logical));
    assert_eq!(totals.union.logical, zero(SizeBasis::Logical));
    assert_eq!(totals.overlap_count, 0);
    let reconciled = read.reconcile(|| false).map_err(err)?;
    assert_eq!(reconciled.observed_files, 0);
    assert!(reconciled.source_coverage_complete);
    Ok(())
}

#[test]
fn no_rules_leave_every_file_unclassified() -> Result<(), String> {
    let (version, scope) = fixture()?;
    let read = classify(&version, Some(&scope), vec![], default())?;
    let summary = read.summary();
    assert_eq!(
        (
            summary.scope_files,
            summary.matched,
            summary.unclassified,
            summary.undecidable
        ),
        (3, 0, 3, 0)
    );
    assert!(summary.groups.is_empty() && summary.reconciled);
    assert!(read.rule_unavailable().is_empty() && read.predicates().is_empty());
    let totals = read.totals(&[], || false).map_err(err)?;
    assert!(totals.groups.is_empty());
    assert_eq!(totals.unclassified, totals.scope);
    assert_eq!(
        read.totals(&[0], || false).map(|_| ()),
        Err(Error::InvalidSelection)
    );
    Ok(())
}

fn unreadable_size_result() -> Result<(Arc<ResultVersion>, ItemRef), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(err)?;
    build.add_directory(vec![]).map_err(err)?;
    for name in ["x", "y", "z"] {
        build
            .add_file(Observation {
                path: vec![bytes(name)],
                size: None,
            })
            .map_err(err)?;
    }
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let root = item_ref(&version, 0);
    Ok((version, root))
}

#[test]
fn files_whose_value_is_unknown_are_undecidable_and_never_unclassified() -> Result<(), String> {
    let (version, root) = unreadable_size_result()?;
    let rule = Condition::Predicate(Predicate::Size(Basis::Logical, Boundary::AtLeast(1)));
    let read = classify(&version, Some(&root), vec![rule], default())?;
    let summary = read.summary();
    assert_eq!(
        (summary.matched, summary.unclassified, summary.undecidable),
        (0, 0, 3)
    );
    assert_eq!(summary.groups[0].undecidable, 3);
    assert_eq!(
        summary.reason_counts.get(group::REASON_VALUE_UNKNOWN),
        Some(&3)
    );
    assert!(read.rows().all(|(_, row)| row == [Decision::Unknown]));
    assert_eq!(Decision::Unknown.reason_key(), Some("group.value_unknown"));
    let totals = read.totals(&[], || false).map_err(err)?;
    assert_eq!(totals.undecidable.logical.amount, SizeAmount::Unknown);
    assert_eq!(
        totals.unclassified.logical,
        SizeValue {
            evidence: SizeEvidence::Measured,
            ..size(SizeBasis::Logical, 0)
        }
    );
    Ok(())
}

#[test]
fn an_unregistered_predicate_makes_only_its_rule_unavailable() -> Result<(), String> {
    let (version, scope) = fixture()?;
    let rules = vec![starts_with("a-"), absent()];
    let read = classify(&version, Some(&scope), rules, default())?;
    assert_eq!(read.rule_unavailable(), [false, true]);
    let rows: Vec<_> = read.rows().map(|(_, row)| row.to_vec()).collect();
    assert_eq!(
        rows,
        [
            vec![Decision::Match, Decision::RuleUnavailable],
            vec![Decision::NoMatch, Decision::RuleUnavailable],
            vec![Decision::NoMatch, Decision::RuleUnavailable],
        ]
    );
    assert_eq!(
        Decision::RuleUnavailable.reason_key(),
        Some("group.rule_unevaluable")
    );
    let summary = read.summary();
    assert_eq!(
        (summary.matched, summary.undecidable, summary.unclassified),
        (1, 2, 0)
    );
    assert_eq!(summary.groups[0].members, 1);
    assert_eq!(summary.groups[1].members, 0);
    assert_eq!(summary.groups[1].undecidable, 3);
    assert_eq!(
        summary.reason_counts.get(group::REASON_RULE_UNEVALUABLE),
        Some(&2)
    );
    assert_eq!(
        read.predicates(),
        [("pred.absent".to_owned(), 1), ("pred.name".to_owned(), 1)]
    );
    Ok(())
}

#[test]
fn a_registered_predicate_cannot_stand_in_for_a_built_in_one() -> Result<(), String> {
    let (version, scope) = fixture()?;
    let read = classify(
        &version,
        Some(&scope),
        vec![registered("pred.name")],
        default(),
    )?;
    assert_eq!(read.rule_unavailable(), [true]);
    Ok(())
}

#[test]
fn reconciliation_uses_all_rules_and_counts_each_reason_once_per_undecidable_file()
-> Result<(), String> {
    let (version, scope) = fixture()?;
    let rules = vec![
        starts_with("a-"),
        starts_with("b-"),
        registered("pred.unknown"),
        absent(),
        absent(),
    ];
    let read = classify(&version, Some(&scope), rules, default())?;
    let recount = read.reconcile(|| false).map_err(err)?;
    assert_eq!(
        (
            recount.observed_files,
            recount.matched_files,
            recount.unclassified_files,
            recount.undecidable_files
        ),
        (3, 2, 0, 1)
    );
    assert!(recount.source_coverage_complete);
    assert_eq!(
        recount
            .known_reason_counts
            .get(group::REASON_RULE_UNEVALUABLE),
        Some(&1)
    );
    assert_eq!(
        recount.known_reason_counts.get(group::REASON_VALUE_UNKNOWN),
        Some(&1)
    );
    assert_eq!(recount.unattributed_files, 1);
    assert_eq!(read.summary().reason_counts.len(), 2);
    assert_eq!(read.summary().groups[2].undecidable, 3);
    assert_eq!(read.summary().overlap, 0);
    assert_eq!(read.reconcile(|| true).map(|_| ()), Err(Error::Cancelled));
    Ok(())
}

#[test]
fn partial_results_and_untraversed_links_are_not_exhaustive_coverage() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(err)?;
    build.add_directory(vec![]).map_err(err)?;
    build
        .add_file(Observation {
            path: vec![bytes("file")],
            size: None,
        })
        .map_err(err)?;
    let partial = build.publish_partial(id(2)).map_err(err)?;
    let root = item_ref(&partial, 0);
    let read = classify(&partial, Some(&root), vec![], default())?;
    let recount = read.reconcile(|| false).map_err(err)?;
    assert_eq!((recount.observed_files, recount.unclassified_files), (1, 1));
    assert!(!recount.source_coverage_complete);
    assert!(recount.known_reason_counts.is_empty() && recount.unattributed_files == 0);
    let sink = IngestSink::default();
    let mut build = sink.open(id(4)).map_err(err)?;
    build.add_directory(vec![]).map_err(err)?;
    build
        .add_untraversed_link(vec![bytes("link")])
        .map_err(err)?;
    let version = build
        .seal(id(5), ResultCompleteness::Complete)
        .map_err(err)?;
    let read = classify(&version, None, vec![], default())?;
    let recount = read.reconcile(|| false).map_err(err)?;
    assert_eq!(recount.observed_files, 0);
    assert!(!recount.source_coverage_complete);
    assert!(read.summary().reconciled);
    Ok(())
}

#[test]
fn only_a_folder_of_the_same_version_can_be_a_scope() -> Result<(), String> {
    let (version, scope) = fixture()?;
    assert_eq!(group::scope_file_count(&version, Some(&scope)), Ok(3));
    assert_eq!(group::scope_file_count(&version, None), Ok(4));
    let file = version
        .items()
        .iter()
        .find(|item| item.name == b"z-other")
        .ok_or("file")?;
    let candidates = [
        item_ref(&version, file.item_no),
        ItemRef {
            result_version: id(9),
            item_no: scope.item_no,
        },
        ItemRef {
            result_version: scope.result_version.clone(),
            item_no: u32::MAX,
        },
    ];
    for candidate in candidates {
        assert_eq!(
            group::scope_file_count(&version, Some(&candidate)),
            Err(Error::InvalidRoot)
        );
        assert!(matches!(
            group::classify_scope(
                Arc::clone(&version),
                Some(&candidate),
                &registry()?,
                vec![],
                default(),
                &Recording::default(),
            ),
            Err(Error::InvalidRoot)
        ));
    }
    Ok(())
}

#[test]
fn a_rule_beyond_the_condition_limits_is_rejected() -> Result<(), String> {
    let (version, scope) = fixture()?;
    let mut deep = starts_with("a");
    for _ in 0..70 {
        deep = Condition::Not(Box::new(deep));
    }
    let outcome = group::classify_scope(
        Arc::clone(&version),
        Some(&scope),
        &registry()?,
        vec![deep],
        default(),
        &Recording::default(),
    );
    assert!(matches!(outcome, Err(Error::InvalidRule(_))));
    Ok(())
}

#[test]
fn a_later_publication_does_not_change_a_classification_of_the_older_version() -> Result<(), String>
{
    let sink = IngestSink::default();
    let mut first = sink.open(id(1)).map_err(err)?;
    first.add_directory(vec![]).map_err(err)?;
    add_file(&mut first, 0, &["a-old"], Some(8), 8, 1)?;
    let old = first
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(err)?;
    let read = classify(&old, None, vec![starts_with("a-")], default())?;
    let before = read.totals(&[], || false).map_err(err)?;
    let mut second = sink.open(id(1)).map_err(err)?;
    second.add_directory(vec![]).map_err(err)?;
    add_file(&mut second, 0, &["a-new"], Some(500), 500, 2)?;
    add_file(&mut second, 0, &["b-new"], Some(500), 500, 3)?;
    let newer = second
        .seal(id(3), ResultCompleteness::Complete)
        .map_err(err)?;
    assert!(Arc::ptr_eq(read.version(), &old));
    assert_eq!(read.totals(&[], || false).map_err(err)?, before);
    assert_eq!(read.summary().scope_files, 1);
    let current = classify(&newer, None, vec![starts_with("a-")], default())?;
    assert_eq!(current.summary().scope_files, 2);
    assert!(!Arc::ptr_eq(read.version(), current.version()));
    let stale = ItemRef {
        result_version: id(3),
        item_no: 1,
    };
    assert_eq!(
        read.explain(&stale, &registry()?).map(|_| ()),
        Err(Error::InvalidItem)
    );
    Ok(())
}
