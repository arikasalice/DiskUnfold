use std::{num::NonZeroUsize, sync::Arc};

use du_core_contracts::{Id, result::ResultCompleteness};

use super::*;
use crate::{
    IngestSink, Observation,
    group::{Control, Decision, Parallelism, classify_scope},
    query::{Condition, NameEncoding, NameTest, Predicate, Registry},
};

struct Quiet;

impl Control for Quiet {
    fn cancelled(&self) -> bool {
        false
    }
    fn progress(&self, _: u64) {}
}

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn classification(files: &[&str], rules: Vec<Condition>) -> Result<Classification, String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for name in files {
        build
            .add_file(Observation {
                path: vec![name.as_bytes().to_vec()],
                size: None,
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    classify_scope(
        version,
        None,
        &Registry::baseline(),
        rules,
        Parallelism::default(),
        &Quiet,
    )
    .map_err(|e| format!("{e:?}"))
}

fn starts_with(prefix: &str) -> Condition {
    Condition::Predicate(Predicate::Name(
        NameTest::StartsWith(prefix.as_bytes().to_vec()),
        NameEncoding::Bytes,
    ))
}

fn valid() -> Result<Arc<Classification>, String> {
    classification(&["a", "b", "c"], vec![starts_with("a")]).map(Arc::new)
}

#[test]
fn full_store_evicts_the_least_recently_used_entry() -> Result<(), String> {
    let capacity = NonZeroUsize::new(2).ok_or("capacity")?;
    let store = ClassificationStore::with_capacity(capacity);
    let (first, second, third) = (id(11)?, id(12)?, id(13)?);
    store
        .register(first.clone(), valid()?)
        .map_err(|e| format!("{e:?}"))?;
    store
        .register(second.clone(), valid()?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(store.get(&first).is_some());
    store
        .register(third.clone(), valid()?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(store.get(&second).is_none());
    assert!(store.get(&first).is_some());
    assert!(store.get(&third).is_some());
    Ok(())
}

#[test]
fn default_capacity_is_eight_and_registering_an_id_again_replaces_it() -> Result<(), String> {
    let store = ClassificationStore::default();
    let ids = (20..29).map(id).collect::<Result<Vec<_>, _>>()?;
    for held in &ids {
        store
            .register(held.clone(), valid()?)
            .map_err(|e| format!("{e:?}"))?;
    }
    assert!(store.get(&ids[0]).is_none());
    assert!(ids[1..].iter().all(|held| store.get(held).is_some()));
    let replacement = valid()?;
    store
        .register(ids[1].clone(), Arc::clone(&replacement))
        .map_err(|e| format!("{e:?}"))?;
    let stored = store.get(&ids[1]).ok_or("replaced entry")?;
    assert!(Arc::ptr_eq(&stored, &replacement));
    Ok(())
}

#[test]
fn counts_that_do_not_reconcile_are_rejected_without_storing() -> Result<(), String> {
    let store = ClassificationStore::default();
    let mut broken_count = classification(&["a", "b"], vec![starts_with("a")])?;
    broken_count.summary.unclassified += 1;
    let mut broken_flag = classification(&["a", "b"], vec![starts_with("a")])?;
    broken_flag.summary.reconciled = false;
    let mut short = classification(&["a", "b"], vec![starts_with("a")])?;
    short.decisions.pop();
    let mut misplaced = classification(&["a", "b"], vec![starts_with("a")])?;
    misplaced.decisions = vec![Decision::NoMatch; 2];
    for (n, broken) in [broken_count, broken_flag, short, misplaced]
        .into_iter()
        .enumerate()
    {
        let held = id(40 + u8::try_from(n).map_err(|e| e.to_string())?)?;
        assert_eq!(
            store.register(held.clone(), Arc::new(broken)),
            Err(Error::InconsistentMembership)
        );
        assert!(store.get(&held).is_none());
    }
    Ok(())
}

#[test]
fn retain_removes_entries_whose_source_is_gone() -> Result<(), String> {
    let store = ClassificationStore::default();
    let (keep, drop_me) = (id(50)?, id(51)?);
    store
        .register(keep.clone(), valid()?)
        .map_err(|e| format!("{e:?}"))?;
    let other = Arc::new(classification(&["x"], vec![starts_with("x")])?);
    store
        .register(drop_me.clone(), other)
        .map_err(|e| format!("{e:?}"))?;
    store.retain(|held| held.summary().scope_files == 3);
    assert!(store.get(&keep).is_some());
    assert!(store.get(&drop_me).is_none());
    Ok(())
}
