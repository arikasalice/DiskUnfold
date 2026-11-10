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
