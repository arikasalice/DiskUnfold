//! Covers initial snapshot admission and recovery before live changes.
use super::*;

#[test]
fn batch_waits_for_a_successfully_applied_initial_snapshot() {
    let mut l = Ledger::new([1; 16]);
    l.register::<()>(definition(3)).unwrap();
    let mut sink = Sink::default();
    assert_eq!(apply(&mut l, &batch(3, 1), &mut sink), Ok(Outcome::Resync));
    assert_eq!(sink.count, 0);
    assert_eq!(l.unit(&[3; 16]).unwrap().last_change_sequence, 0);
    sink.fail = true;
    assert_eq!(
        snap(&mut l, &snapshot(3, 0, 0), &mut sink),
        Err(Error::Apply("failed"))
    );
    sink.fail = false;
    assert_eq!(apply(&mut l, &batch(3, 1), &mut sink), Ok(Outcome::Resync));
    assert_eq!(
        snap(&mut l, &snapshot(3, 0, 0), &mut sink),
        Ok(Outcome::Applied)
    );
    assert_eq!(apply(&mut l, &batch(3, 1), &mut sink), Ok(Outcome::Applied));
}

#[test]
fn initial_snapshot_cut_sets_the_first_permitted_change_sequence() {
    let mut l = Ledger::new([1; 16]);
    l.register::<()>(definition(3)).unwrap();
    let mut sink = Sink::default();
    assert_eq!(
        snap(&mut l, &snapshot(3, 5, 10), &mut sink),
        Ok(Outcome::Applied)
    );
    assert_eq!(
        apply(&mut l, &batch(3, 5), &mut sink),
        Ok(Outcome::Duplicate)
    );
    assert_eq!(apply(&mut l, &batch(3, 6), &mut sink), Ok(Outcome::Applied));
    assert_eq!(l.unit(&[3; 16]).unwrap().last_change_sequence, 6);
}

#[test]
fn snapshot_cut_duplicate_and_failed_change_keep_the_initial_evidence() {
    let mut l = Ledger::new([1; 16]);
    l.register::<()>(definition(3)).unwrap();
    let mut sink = Sink::default();
    let mut cut = snapshot(3, 5, 10);
    cut.gaps.push(gap(9));
    assert_eq!(snap(&mut l, &cut, &mut sink), Ok(Outcome::Applied));
    let committed = l.unit(&[3; 16]).unwrap().clone();
    assert_eq!(snap(&mut l, &cut, &mut sink), Ok(Outcome::Duplicate));
    assert_eq!(sink.count, 1);

    let mut change_batch = batch(3, 6);
    change_batch.changes.push(change(6));
    change_batch.gaps.push(gap(10));
    sink.fail = true;
    assert_eq!(
        apply(&mut l, &change_batch, &mut sink),
        Err(Error::Apply("failed"))
    );
    let after = l.unit(&[3; 16]).unwrap();
    assert_eq!(after.last_change_sequence, committed.last_change_sequence);
    assert_eq!(after.generation, committed.generation);
    assert_eq!(after.gaps, committed.gaps);
    assert!(after.needs_resync);
    assert_eq!(sink.gaps[&3], committed.gaps);
    assert!(sink.independent.is_empty());
    sink.fail = false;
    assert_eq!(apply(&mut l, &batch(3, 7), &mut sink), Ok(Outcome::Resync));
    assert_eq!(sink.count, 1);

    let mut fresh = cut.clone();
    fresh.observed_at += 1;
    fresh.last_observed = Some(fresh.observed_at);
    assert_eq!(snap(&mut l, &fresh, &mut sink), Ok(Outcome::Applied));
    assert_eq!(
        apply(&mut l, &change_batch, &mut sink),
        Ok(Outcome::Applied)
    );
    assert_eq!(l.unit(&[3; 16]).unwrap().last_change_sequence, 6);
    assert_eq!(sink.independent, vec![6]);
    assert!(sink.gaps[&3].contains_key(&9));
    assert!(sink.gaps[&3].contains_key(&10));
}

#[test]
fn different_scope_never_initializes_or_forwards_the_selected_scope() {
    let mut l = Ledger::new([1; 16]);
    let mut wrong_definition = definition(3);
    wrong_definition.scope_revision = [2; 16];
    assert_eq!(l.register::<()>(wrong_definition), Err(Error::WrongScope));
    l.register::<()>(definition(3)).unwrap();
    let mut sink = Sink::default();
    let mut other_snapshot = snapshot(3, 0, 0);
    other_snapshot.definition.scope_revision = [2; 16];
    assert_eq!(
        snap(&mut l, &other_snapshot, &mut sink),
        Ok(Outcome::ReleasedScope)
    );
    let mut other_batch = batch(3, 1);
    other_batch.context.scope_revision = [2; 16];
    assert_eq!(
        apply(&mut l, &other_batch, &mut sink),
        Ok(Outcome::ReleasedScope)
    );
    assert_eq!(sink.count, 0);
    let unit = l.unit(&[3; 16]).unwrap();
    assert_eq!(unit.last_change_sequence, 0);
    assert!(unit.snapshot.is_none());
}
