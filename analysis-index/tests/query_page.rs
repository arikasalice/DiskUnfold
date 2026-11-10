use du_analysis_index::query::page::{PageError, ResultSets, Total};
use du_analysis_index::query::{
    self, Basis, Condition, Kind, NameEncoding, NameTest, Predicate, Registry, Sort,
};
use du_analysis_index::{BuildError, IngestSink, Observation, ResultVersion};
use du_core_contracts::Id;
use du_core_contracts::result::ResultCompleteness;
use du_core_contracts::{ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue};
use std::sync::Arc;

fn id(n: u64) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn error(e: BuildError) -> String {
    format!("{e:?}")
}
fn fixture(count: usize) -> Result<(IngestSink, Arc<ResultVersion>), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(error)?;
    build.add_directory(vec![]).map_err(error)?;
    for n in 0..count {
        build
            .add(Observation {
                path: vec![format!("{n:05}").into_bytes()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: ByteSize::from(10),
                    },
                }),
            })
            .map_err(error)?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    Ok((sink, version))
}
fn result(version: Arc<ResultVersion>) -> Result<query::QueryResult, String> {
    query::run(
        &Registry::baseline(),
        version,
        &Condition::All(vec![]),
        Sort {
            id: "sort.size",
            revision: 1,
            basis: Basis::Logical,
            descending: true,
        },
        || false,
    )
    .map_err(|e| format!("{e:?}"))
}

#[test]
fn refinement_reads_only_admitted_parent_rows_and_keeps_both_conditions() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(error)?;
    build.add_directory(vec![]).map_err(error)?;
    for name in [vec![b'a', 0], vec![b'b', 0], vec![b'x']] {
        build
            .add_file(Observation {
                path: vec![name],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: ByteSize::from(1),
                    },
                }),
            })
            .map_err(error)?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    let sets = ResultSets::new(sink);
    let registry = Registry::baseline();
    let sort = || Sort {
        id: "sort.path",
        revision: 1,
        basis: Basis::Logical,
        descending: false,
    };
    let upper = Condition::Predicate(Predicate::Name(
        NameTest::Equal(vec![b'a', 0]),
        NameEncoding::WindowsUtf16Le,
    ));
    let added = Condition::Predicate(Predicate::Kind(Kind::File));
    let parent =
        query::run(&registry, version, &upper, sort(), || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(parent.unknown_count, 1);
    assert_eq!(parent.items.len(), 1);
    sets.register(id(3)?, parent)
        .map_err(|e| format!("{e:?}"))?;
    let parent = sets
        .open_parent(&id(3)?, &id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    sets.release(&id(3)?).map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        sets.open_parent(&id(3)?, &id(2)?),
        Err(PageError::StaleVersion)
    ));
    let refined = query::run_in_result(&registry, &parent, &added, sort(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(refined.items, parent.items);
    assert_eq!(refined.condition, Condition::All(vec![upper, added]));
    sets.register(id(4)?, refined)
        .map_err(|e| format!("{e:?}"))?;
    let page = sets
        .open_page(&id(4)?, &id(2)?, 0, None)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.metadata().unknown_count, 0);
    assert!(page.metadata().inherited_uncertainty);
    assert_eq!(page.metadata().total, Total::LowerBound(1));
    Ok(())
}

#[test]
fn pages_cover_tied_items_once_and_validate_boundaries() -> Result<(), Box<dyn std::error::Error>> {
    let (sink, version) = fixture(2200)?;
    let store = ResultSets::new(sink);
    let result = result(version)?;
    let expected = result.items.clone();
    store
        .register(id(3)?, result)
        .map_err(|e| format!("{e:?}"))?;
    let first = store
        .open_page(&id(3)?, &id(2)?, 0, None)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.items().len(), 200);
    assert_eq!(first.metadata().total, Total::Exact(2201));
    assert_eq!(first.metadata().sort_strategy, &("sort.size".into(), 1));
    assert!(first.metadata().sort_descending);
    let mut collected = Vec::new();
    let mut start = 0_u64;
    loop {
        let page = store
            .open_page(&id(3)?, &id(2)?, start, Some(1000))
            .map_err(|e| format!("{e:?}"))?;
        collected.extend_from_slice(page.items());
        let next = page.next_position() as u64;
        if next == start || next == expected.len() as u64 {
            break;
        }
        start = next;
    }
    assert_eq!(collected, expected);
    assert!(
        store
            .open_page(&id(3)?, &id(2)?, 2201, Some(1))
            .map_err(|e| format!("{e:?}"))?
            .items()
            .is_empty()
    );
    for (start, count) in [
        (2202, Some(1)),
        (0, Some(0)),
        (0, Some(1001)),
        (u64::MAX, Some(1)),
    ] {
        assert!(matches!(
            store.open_page(&id(3)?, &id(2)?, start, count),
            Err(PageError::InvalidRange)
        ));
    }
    Ok(())
}

#[test]
fn replacement_and_release_reject_new_pages_but_preserve_admitted_page()
-> Result<(), Box<dyn std::error::Error>> {
    let (sink, version) = fixture(3)?;
    let store = ResultSets::new(sink.clone());
    store
        .register(id(3)?, result(version.clone())?)
        .map_err(|e| format!("{e:?}"))?;
    let page = store
        .open_page(&id(3)?, &id(2)?, 0, Some(2))
        .map_err(|e| format!("{e:?}"))?;
    let mut build = sink.open(id(1)?).map_err(error)?;
    build.add_directory(vec![]).map_err(error)?;
    build
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(error)?;
    assert!(matches!(
        store.open_page(&id(3)?, &id(2)?, 2, Some(2)),
        Err(PageError::StaleVersion)
    ));
    assert!(matches!(
        store.register(id(5)?, result(version)?),
        Err(PageError::StaleVersion)
    ));
    store
        .release_source(&id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.items().len(), 2);
    assert_eq!(page.metadata().version, &id(2)?);
    assert!(matches!(
        store.open_page(&id(3)?, &id(2)?, 0, None),
        Err(PageError::StaleVersion)
    ));
    Ok(())
}

#[test]
fn lru_eviction_ids_and_duplicate_rows_are_guarded() -> Result<(), Box<dyn std::error::Error>> {
    let (sink, version) = fixture(3)?;
    let store = ResultSets::new(sink);
    for n in 10..26 {
        store
            .register(id(n)?, result(version.clone())?)
            .map_err(|e| format!("{e:?}"))?;
    }
    let oldest_page = store
        .open_page(&id(10)?, &id(2)?, 0, None)
        .map_err(|e| format!("{e:?}"))?;
    for n in 11..26 {
        store
            .open_page(&id(n)?, &id(2)?, 0, None)
            .map_err(|e| format!("{e:?}"))?;
    }
    store
        .register(id(26)?, result(version.clone())?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        store.open_page(&id(10)?, &id(2)?, 0, None),
        Err(PageError::StaleVersion)
    ));
    assert_eq!(oldest_page.items().len(), 4);
    assert!(matches!(
        store.register(id(10)?, result(version.clone())?),
        Err(PageError::DuplicateId)
    ));
    let mut invalid = result(version)?;
    invalid.items.push(invalid.items[0].clone());
    assert!(matches!(
        store.register(id(27)?, invalid),
        Err(PageError::InvalidResult)
    ));
    Ok(())
}

#[test]
fn incomplete_source_and_unknown_predicate_produce_lower_bound_total()
-> Result<(), Box<dyn std::error::Error>> {
    let (sink, version) = fixture(3)?;
    let store = ResultSets::new(sink);
    let condition = Condition::Predicate(query::Predicate::Size(
        Basis::Allocated,
        query::Boundary::AtLeast(1),
    ));
    let result = query::run(
        &Registry::baseline(),
        version,
        &condition,
        Sort {
            id: "sort.path",
            revision: 1,
            basis: Basis::Allocated,
            descending: false,
        },
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    store
        .register(id(3)?, result)
        .map_err(|e| format!("{e:?}"))?;
    let page = store
        .open_page(&id(3)?, &id(2)?, 0, None)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.metadata().total, Total::LowerBound(0));
    assert!(page.metadata().unknown_count > 0);
    Ok(())
}

#[test]
fn explicit_release_preserves_admitted_page_and_rejects_different_source_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let (sink, version) = fixture(2)?;
    let (_, other_version) = fixture(2)?;
    let store = ResultSets::new(sink);
    assert!(matches!(
        store.register(id(6)?, result(other_version)?),
        Err(PageError::StaleVersion)
    ));
    store
        .register(id(3)?, result(version)?)
        .map_err(|e| format!("{e:?}"))?;
    let page = store
        .open_page(&id(3)?, &id(2)?, 0, None)
        .map_err(|e| format!("{e:?}"))?;
    store.release(&id(3)?).map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        store.open_page(&id(3)?, &id(2)?, 0, None),
        Err(PageError::StaleVersion)
    ));
    assert_eq!(page.items().len(), 3);
    Ok(())
}

#[test]
fn released_source_cannot_register_again_even_if_still_pinned_by_source()
-> Result<(), Box<dyn std::error::Error>> {
    let (sink, version) = fixture(2)?;
    let store = ResultSets::new(sink);
    store
        .register(id(3)?, result(version.clone())?)
        .map_err(|e| format!("{e:?}"))?;
    let page = store
        .open_page(&id(3)?, &id(2)?, 0, None)
        .map_err(|e| format!("{e:?}"))?;
    store
        .release_source(&id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        store.register(id(4)?, result(version)?),
        Err(PageError::StaleVersion)
    ));
    assert!(matches!(
        store.open_page(&id(3)?, &id(2)?, 0, None),
        Err(PageError::StaleVersion)
    ));
    assert_eq!(page.items().len(), 3);
    Ok(())
}

#[test]
fn untraversed_subtree_makes_match_count_a_lower_bound_even_when_query_is_decidable()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(error)?;
    build.add_directory(vec![]).map_err(error)?;
    build
        .add_untraversed_link(vec![b"link".to_vec()])
        .map_err(error)?;
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    let store = ResultSets::new(sink);
    store
        .register(id(3)?, result(version)?)
        .map_err(|e| format!("{e:?}"))?;
    let page = store
        .open_page(&id(3)?, &id(2)?, 0, None)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.metadata().unknown_count, 0);
    assert_eq!(page.metadata().completeness, ResultCompleteness::Missing);
    assert_eq!(page.metadata().total, Total::LowerBound(2));
    Ok(())
}

#[test]
fn repeated_refinement_keeps_parent_uncertainty_even_when_empty() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(error)?;
    build.add_directory(vec![]).map_err(error)?;
    for name in [vec![b'a', 0], vec![b'x']] {
        build
            .add_file(Observation {
                path: vec![name],
                size: None,
            })
            .map_err(error)?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    let registry = Registry::baseline();
    let sort = || Sort {
        id: "sort.path",
        revision: 1,
        basis: Basis::Logical,
        descending: false,
    };
    let parent = query::run(
        &registry,
        version,
        &Condition::Predicate(Predicate::Name(
            NameTest::Equal(vec![b'a', 0]),
            NameEncoding::WindowsUtf16Le,
        )),
        sort(),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(parent.unknown_count, 1);
    let empty = query::run_in_result(&registry, &parent, &Condition::Any(vec![]), sort(), || {
        false
    })
    .map_err(|e| format!("{e:?}"))?;
    assert!(empty.items.is_empty());
    let final_result =
        query::run_in_result(&registry, &empty, &Condition::All(vec![]), sort(), || false)
            .map_err(|e| format!("{e:?}"))?;
    let sets = ResultSets::new(sink);
    sets.register(id(3)?, final_result)
        .map_err(|e| format!("{e:?}"))?;
    let page = sets
        .open_page(&id(3)?, &id(2)?, 0, None)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.metadata().unknown_count, 0);
    assert!(page.metadata().inherited_uncertainty);
    assert_eq!(page.metadata().total, Total::LowerBound(0));
    Ok(())
}

#[test]
fn cancelled_empty_refinement_does_not_produce_a_result() -> Result<(), String> {
    let (_, version) = fixture(1)?;
    let registry = Registry::baseline();
    let sort = || Sort {
        id: "sort.path",
        revision: 1,
        basis: Basis::Logical,
        descending: false,
    };
    let empty = query::run(&registry, version, &Condition::Any(vec![]), sort(), || {
        false
    })
    .map_err(|e| format!("{e:?}"))?;
    assert!(empty.items.is_empty());
    assert!(matches!(
        query::run_in_result(&registry, &empty, &Condition::All(vec![]), sort(), || true),
        Err(query::Error::Cancelled)
    ));
    Ok(())
}
