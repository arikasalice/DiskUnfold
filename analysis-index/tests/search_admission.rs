use du_analysis_index::{
    IngestSink, Observation,
    query::{
        self, Basis, Condition, Error, Predicate, Record, Registry, Sort, Truth,
        page::{PageError, ResultSets},
    },
};
use du_core_contracts::{Id, result::ResultCompleteness};
use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn sort() -> Sort<'static> {
    Sort {
        id: "sort.path",
        revision: 1,
        basis: Basis::Logical,
        descending: false,
    }
}
fn fixture() -> Result<(IngestSink, Arc<ResultSets>), String> {
    let store = IngestSink::default();
    let mut scan = store.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    scan.add_file(Observation {
        path: vec![b"file".to_vec()],
        size: None,
    })
    .map_err(|e| format!("{e:?}"))?;
    scan.seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok((store.clone(), Arc::new(ResultSets::new(store))))
}
fn yes(_: &Record<'_>, _: &Predicate) -> Truth {
    Truth::Match
}
fn operand(value: &[u8]) -> bool {
    value == b"ok"
}

#[test]
fn preparation_checks_registered_operands_and_sort_without_items() -> Result<(), String> {
    let mut registry = Registry::baseline();
    registry
        .register_checked_predicate("pred.test", 1, yes, operand)
        .map_err(|e| format!("{e:?}"))?;
    let condition = |value: &[u8], revision| {
        Condition::Predicate(Predicate::Registered {
            id: "pred.test".into(),
            revision,
            operand: value.to_vec(),
        })
    };
    assert_eq!(
        registry.prepare_search(&condition(b"ok", 1), &[sort()]),
        Ok(())
    );
    assert_eq!(
        registry.prepare_search(&condition(b"bad", 1), &[sort()]),
        Err(Error::InvalidCondition)
    );
    assert_eq!(
        registry.prepare_search(&condition(b"ok", 2), &[sort()]),
        Err(Error::UnknownPredicate)
    );
    assert_eq!(
        registry.prepare_search(&condition(b"ok", 1), &[]),
        Err(Error::UnknownSort)
    );
    Ok(())
}

#[test]
fn admitted_first_page_survives_concurrent_source_replacement() -> Result<(), String> {
    let (store, sets) = fixture()?;
    let admission = sets
        .admit_search(Some(&id(2)?), None)
        .map_err(|e| format!("{e:?}"))?;
    let result = query::run(
        &Registry::baseline(),
        admission.version(),
        &Condition::All(vec![]),
        sort(),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let candidate = sets
        .validate_search_candidate(admission, result, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || -> Result<(), String> {
        let mut next = store.open(id(3)?).map_err(|e| format!("{e:?}"))?;
        next.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        next.seal(id(4)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        tx.send(()).map_err(|e| format!("{e:?}"))
    });
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("{e:?}"))?;
    let page = sets
        .register_admitted_search(id(5)?, candidate)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.query_ref(), &id(5)?);
    assert_eq!(page.items().len(), 1);
    assert!(matches!(
        sets.open_page(&id(5)?, &id(2)?, 0, Some(1)),
        Err(PageError::StaleVersion)
    ));
    worker.join().map_err(|_| "worker panicked")??;
    Ok(())
}

#[test]
fn parent_and_source_are_admitted_together() -> Result<(), String> {
    let (_, sets) = fixture()?;
    let admission = sets
        .admit_search(None, None)
        .map_err(|e| format!("{e:?}"))?;
    let result = query::run(
        &Registry::baseline(),
        admission.version(),
        &Condition::All(vec![]),
        sort(),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let checked = sets
        .validate_search_candidate(admission, result, None)
        .map_err(|e| format!("{e:?}"))?;
    sets.register_admitted_search(id(5)?, checked)
        .map_err(|e| format!("{e:?}"))?;
    assert!(sets.admit_search(Some(&id(2)?), Some(&id(5)?)).is_ok());
    assert!(matches!(
        sets.admit_search(Some(&id(9)?), Some(&id(5)?)),
        Err(PageError::StaleVersion)
    ));
    Ok(())
}

#[test]
fn refinement_cannot_publish_rows_outside_the_admitted_parent() -> Result<(), String> {
    let (_, sets) = fixture()?;
    let registry = Registry::baseline();
    let source = sets
        .admit_search(None, None)
        .map_err(|error| format!("{error:?}"))?;
    let empty_parent = query::run(
        &registry,
        source.version(),
        &Condition::Any(vec![]),
        sort(),
        || false,
    )
    .map_err(|error| format!("{error:?}"))?;
    let parent_ref = id(5)?;
    sets.register(parent_ref.clone(), empty_parent)
        .map_err(|error| format!("{error:?}"))?;
    let child = sets
        .admit_search(None, Some(&parent_ref))
        .map_err(|error| format!("{error:?}"))?;
    let all = Condition::All(vec![]);
    let widened = query::run(&registry, child.version(), &all, sort(), || false)
        .map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        sets.validate_search_candidate(child.clone(), widened, None),
        Err(PageError::InvalidResult)
    ));
    // Rejecting an invalid candidate must leave the already admitted parent readable.
    let parent_page = sets
        .open_page(&parent_ref, &id(2)?, 0, None)
        .map_err(|error| format!("{error:?}"))?;
    assert!(parent_page.items().is_empty());

    sets.release(&parent_ref)
        .map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        sets.admit_search(None, Some(&parent_ref)),
        Err(PageError::StaleVersion)
    ));
    let parent = child.parent().ok_or("missing admitted parent")?;
    let empty_child = query::run_in_result(&registry, parent, &all, sort(), || false)
        .map_err(|error| format!("{error:?}"))?;
    let candidate = sets
        .validate_search_candidate(child, empty_child, None)
        .map_err(|error| format!("{error:?}"))?;
    let page = sets
        .register_admitted_search(id(6)?, candidate)
        .map_err(|error| format!("{error:?}"))?;
    assert!(page.items().is_empty());
    assert!(!page.has_next());
    Ok(())
}

fn no(_: &Record<'_>, _: &Predicate) -> Truth {
    Truth::NoMatch
}

#[test]
fn prepared_job_keeps_condition_sort_registry_and_source_snapshot() -> Result<(), String> {
    use du_analysis_index::query::{
        SortSpec,
        prepared::{PreparedSearch, SearchInput},
    };
    let (store, sets) = fixture()?;
    let mut registry = Registry::baseline();
    registry
        .register_checked_predicate("pred.test", 1, yes, operand)
        .map_err(|e| format!("{e:?}"))?;
    let condition = Condition::Predicate(Predicate::Registered {
        id: "pred.test".into(),
        revision: 1,
        operand: b"ok".to_vec(),
    });
    let mut sorts = vec![SortSpec::from(&sort())];
    let admission = sets
        .admit_search(None, None)
        .map_err(|e| format!("{e:?}"))?;
    let prepared = PreparedSearch::new(
        sets.clone(),
        admission,
        &registry,
        SearchInput {
            condition: condition.clone(),
            sorts: sorts.clone(),
            count: Some(1),
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    registry
        .register_checked_predicate("pred.test", 2, no, operand)
        .map_err(|e| format!("{e:?}"))?;
    sorts[0].descending = true;
    let mut replacement = store.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let candidate = thread::spawn(move || prepared.evaluate(|| false))
        .join()
        .map_err(|_| "worker panicked")?
        .map_err(|e| format!("{e:?}"))?;
    let page = sets
        .register_admitted_search(id(5)?, candidate)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.items().len(), 1);
    assert!(page.has_next());
    assert_eq!(page.metadata().condition, &condition);
    assert!(!page.metadata().sort_descending);
    assert_eq!(
        page.metadata().predicate_strategies,
        &[("pred.test".into(), 1)]
    );
    assert_eq!(page.metadata().version, &id(2)?);
    assert!(matches!(
        sets.open_page(&id(5)?, &id(2)?, 0, Some(1)),
        Err(PageError::StaleVersion)
    ));
    Ok(())
}

#[test]
fn combined_parent_depth_is_rejected_before_evaluation() -> Result<(), String> {
    use du_analysis_index::query::{
        SortSpec,
        prepared::{PreparedSearch, PreparedSearchError, SearchInput},
    };
    let (_, sets) = fixture()?;
    let registry = Registry::baseline();
    let mut deep = Condition::All(vec![]);
    for _ in 0..64 {
        deep = Condition::Not(Box::new(deep));
    }
    let admission = sets
        .admit_search(None, None)
        .map_err(|e| format!("{e:?}"))?;
    let result = query::run(&registry, admission.version(), &deep, sort(), || false)
        .map_err(|e| format!("{e:?}"))?;
    let candidate = sets
        .validate_search_candidate(admission, result, None)
        .map_err(|e| format!("{e:?}"))?;
    sets.register_admitted_search(id(5)?, candidate)
        .map_err(|e| format!("{e:?}"))?;
    let parent = sets
        .admit_search(None, Some(&id(5)?))
        .map_err(|e| format!("{e:?}"))?;
    let result = PreparedSearch::new(
        sets,
        parent,
        &registry,
        SearchInput {
            condition: Condition::All(vec![]),
            sorts: vec![SortSpec::from(&sort())],
            count: None,
        },
    );
    assert!(matches!(
        result,
        Err(PreparedSearchError::Query(Error::InvalidCondition))
    ));
    Ok(())
}

#[test]
fn prepared_refinement_retains_parent_membership_and_pinned_version() -> Result<(), String> {
    use du_analysis_index::query::{
        NameEncoding, NameTest, SortSpec,
        prepared::{PreparedSearch, SearchInput},
    };
    let store = IngestSink::default();
    let mut scan = store.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for name in [b"alpha".as_slice(), b"beta".as_slice()] {
        scan.add_file(Observation {
            path: vec![name.to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    scan.seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let sets = Arc::new(ResultSets::new(store));
    let registry = Registry::baseline();
    let parent_condition = Condition::Predicate(Predicate::Name(
        NameTest::StartsWith(b"a".to_vec()),
        NameEncoding::Bytes,
    ));
    let source = sets
        .admit_search(None, None)
        .map_err(|e| format!("{e:?}"))?;
    let parent = query::run(
        &registry,
        source.version(),
        &parent_condition,
        sort(),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let checked = sets
        .validate_search_candidate(source, parent, None)
        .map_err(|e| format!("{e:?}"))?;
    sets.register_admitted_search(id(5)?, checked)
        .map_err(|e| format!("{e:?}"))?;
    let admission = sets
        .admit_search(Some(&id(2)?), Some(&id(5)?))
        .map_err(|e| format!("{e:?}"))?;
    let prepared = PreparedSearch::new(
        sets.clone(),
        admission,
        &registry,
        SearchInput {
            condition: Condition::All(vec![]),
            sorts: vec![SortSpec::from(&sort())],
            count: None,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let candidate = prepared.evaluate(|| false).map_err(|e| format!("{e:?}"))?;
    let page = sets
        .register_admitted_search(id(6)?, candidate)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.metadata().version, &id(2)?);
    assert_eq!(page.items().len(), 1);
    assert_eq!(page.items()[0].item_no, 1);
    Ok(())
}

#[test]
fn cancelled_prepared_refinement_produces_no_publishable_candidate() -> Result<(), String> {
    use du_analysis_index::query::{
        SortSpec,
        prepared::{PreparedSearch, PreparedSearchError, SearchInput},
    };
    let (_, sets) = fixture()?;
    let registry = Registry::baseline();
    let source = sets
        .admit_search(None, None)
        .map_err(|e| format!("{e:?}"))?;
    let result = query::run(
        &registry,
        source.version(),
        &Condition::All(vec![]),
        sort(),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let checked = sets
        .validate_search_candidate(source, result, None)
        .map_err(|e| format!("{e:?}"))?;
    sets.register_admitted_search(id(5)?, checked)
        .map_err(|e| format!("{e:?}"))?;
    let parent = sets
        .admit_search(None, Some(&id(5)?))
        .map_err(|e| format!("{e:?}"))?;
    let prepared = PreparedSearch::new(
        sets.clone(),
        parent,
        &registry,
        SearchInput {
            condition: Condition::All(vec![]),
            sorts: vec![SortSpec::from(&sort())],
            count: None,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        prepared.evaluate(|| true),
        Err(PreparedSearchError::Query(Error::Cancelled))
    ));
    assert!(matches!(
        sets.open_page(&id(6)?, &id(2)?, 0, None),
        Err(PageError::StaleVersion)
    ));
    assert!(sets.open_page(&id(5)?, &id(2)?, 0, None).is_ok());
    Ok(())
}
