use super::*;

fn bootstrap_scope(ledger: &mut Ledger, id: u8, scope: [u8; 16]) {
    let mut initial = snapshot(id, 0, 0);
    initial.definition.scope_revision = scope;
    initial.sequence = 0;
    ledger
        .apply_snapshot(&initial, &mut |_| Ok::<(), ()>(()))
        .unwrap();
}

#[test]
fn replacing_scope_revision_cannot_reuse_the_old_consumer_lifetime() {
    let mut old = ledger();
    let mut committed = Sink::default();
    let mut old_batch = batch(3, 1);
    old_batch.changes.push(change(1));
    assert_eq!(
        apply(&mut old, &old_batch, &mut committed),
        Ok(Outcome::Applied)
    );
    assert_eq!(committed.independent, vec![1]);

    // A new revision owns a fresh consumer even when the registry reuses the
    // same watch reference. The old scope is retired before the replacement
    // is admitted; committed freshness remains the callback owner's concern.
    old.release();
    let mut replacement = Ledger::new([9; 16]);
    let mut replacement_definition = definition(3);
    replacement_definition.scope_revision = [9; 16];
    replacement.register::<()>(replacement_definition).unwrap();
    bootstrap_scope(&mut replacement, 3, [9; 16]);

    let mut stale = batch(3, 2);
    stale.context.scope_revision = [1; 16];
    assert_eq!(
        apply(&mut replacement, &stale, &mut committed),
        Ok(Outcome::ReleasedScope)
    );
    let mut current = batch(3, 1);
    current.context.scope_revision = [9; 16];
    current.changes.push(change(2));
    assert_eq!(
        apply(&mut replacement, &current, &mut committed),
        Ok(Outcome::Applied)
    );
    assert_eq!(committed.independent, vec![1, 2]);
    assert_eq!(replacement.unit(&[3; 16]).unwrap().last_change_sequence, 1);
}
