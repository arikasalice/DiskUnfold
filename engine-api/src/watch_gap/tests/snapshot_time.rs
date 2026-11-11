use super::*;

#[test]
fn future_snapshot_evidence_never_advances_the_cut_or_reaches_sink() {
    for which in 0..3 {
        let mut ledger = ledger();
        let mut sink = Sink::default();
        let mut first = batch(3, 1);
        first.interval_end = 2;
        first.changes.push(change(1));
        first.gaps.push(gap(1));
        assert_eq!(apply(&mut ledger, &first, &mut sink), Ok(Outcome::Applied));
        let before = ledger.unit(&[3; 16]).unwrap().clone();
        let committed = sink.gaps.clone();
        let count = sink.count;
        let mut cut = snapshot(3, 1, 2);
        cut.gaps.push(gap(1));
        match which {
            0 => cut.last_observed = Some(3),
            1 => {
                cut.gaps[0].start = Some(3);
                cut.gaps[0].end = None;
            }
            _ => cut.gaps[0].end = Some(3),
        }
        assert_eq!(
            snap(&mut ledger, &cut, &mut sink),
            Err(Error::InvalidSnapshot)
        );
        let after = ledger.unit(&[3; 16]).unwrap();
        assert_eq!(after.last_change_sequence, before.last_change_sequence);
        assert_eq!(after.gaps, before.gaps);
        assert!(after.needs_resync);
        assert_eq!(sink.gaps, committed);
        assert_eq!(sink.count, count);
        assert_eq!(sink.independent, vec![1]);
        let mut valid = snapshot(3, 1, 2);
        valid.gaps.push(gap(1));
        assert_eq!(snap(&mut ledger, &valid, &mut sink), Ok(Outcome::Applied));
        assert!(!ledger.unit(&[3; 16]).unwrap().needs_resync);
        assert_eq!(sink.gaps[&3][&1].end, Some(2));
    }
}

#[test]
fn invalid_initial_snapshot_does_not_establish_watch_cut() {
    let mut ledger = Ledger::new([1; 16]);
    ledger.register::<&'static str>(definition(3)).unwrap();
    let mut sink = Sink::default();
    let mut invalid = snapshot(3, 5, 10);
    invalid.gaps.push(Gap {
        start: Some(11),
        end: None,
        ..gap(1)
    });
    assert_eq!(
        snap(&mut ledger, &invalid, &mut sink),
        Err(Error::InvalidSnapshot)
    );
    assert_eq!(sink.count, 0);
    assert_eq!(ledger.unit(&[3; 16]).unwrap().last_change_sequence, 0);
    assert_eq!(
        apply(&mut ledger, &batch(3, 6), &mut sink),
        Ok(Outcome::Resync)
    );
    let mut valid = snapshot(3, 5, 10);
    valid.gaps.push(Gap {
        start: Some(10),
        end: None,
        ..gap(1)
    });
    assert_eq!(snap(&mut ledger, &valid, &mut sink), Ok(Outcome::Applied));
    assert_eq!(ledger.unit(&[3; 16]).unwrap().last_change_sequence, 5);
    assert_eq!(sink.gaps[&3][&1].start, Some(10));
    assert_eq!(
        apply(&mut ledger, &batch(3, 6), &mut sink),
        Ok(Outcome::Applied)
    );
}

#[test]
fn post_cut_gap_is_replayed_even_if_its_time_is_after_snapshot() {
    let mut ledger = ledger();
    let mut sink = Sink::default();
    assert_eq!(
        apply(&mut ledger, &batch(3, 1), &mut sink),
        Ok(Outcome::Applied)
    );
    let mut later = batch(3, 2);
    later.interval_start = 3;
    later.interval_end = 4;
    later.gaps.push(Gap {
        start: Some(3),
        end: Some(4),
        ..gap(2)
    });
    assert_eq!(apply(&mut ledger, &later, &mut sink), Ok(Outcome::Applied));
    let cut = snapshot(3, 1, 2);
    assert_eq!(snap(&mut ledger, &cut, &mut sink), Ok(Outcome::Applied));
    assert_eq!(sink.gaps[&3][&2].end, Some(4));
    assert_eq!(ledger.unit(&[3; 16]).unwrap().last_change_sequence, 2);
}
