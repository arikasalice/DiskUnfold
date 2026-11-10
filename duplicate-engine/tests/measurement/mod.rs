use super::*;

#[test]
fn partial_io_failure_preserves_confirmed_bytes_and_marks_total_inexact() -> Result<(), VerifyError>
{
    let a = item(1, 4);
    let b = item(2, 4);
    let mut port = FakePort::new(&[(a.clone(), b"abcd".to_vec()), (b.clone(), b"abcd".to_vec())]);
    port.max_read = 2;
    if let Some(entry) = port.entries.get_mut(&1) {
        entry.fail_at = Some(2);
    }
    let result = verification::verify(&[a, b], &mut port, Strategies::blake3_v1(), || false)?;
    assert_eq!(result.states.get(&1), Some(&State::ReadFailed));
    assert_eq!(result.full_reads.observed, 6);
    assert!(!result.full_reads.exact);
    assert_eq!(result.sample_reads.observed, 0);
    assert!(result.sample_reads.exact);
    Ok(())
}

#[test]
fn unpaired_item_with_unknown_metadata_stays_deferred() -> Result<(), VerifyError> {
    let mut target = item(1, 2);
    target.snapshot = None;
    let mut port = FakePort::new(&[(target.clone(), b"xx".to_vec())]);
    let (sample, full) = hashers();
    let result = verification::verify(&[target], &mut port, strategies(&sample, &full), || false)?;
    assert_eq!(result.states.get(&1), Some(&State::Unchecked));
    assert!(port.opens.is_empty());
    Ok(())
}

#[test]
fn invalid_eof_count_fails_but_positive_valid_probe_means_changed() -> Result<(), VerifyError> {
    for (invalid, expected) in [(true, State::ReadFailed), (false, State::Changed)] {
        let a = item(1, 3);
        let b = item(2, 3);
        let mut port =
            FakePort::new(&[(a.clone(), b"abcd".to_vec()), (b.clone(), b"abc".to_vec())]);
        if invalid && let Some(entry) = port.entries.get_mut(&1) {
            entry.invalid_at = Some(3);
        }
        let result = verification::verify(&[a, b], &mut port, Strategies::blake3_v1(), || false)?;
        assert_eq!(result.states[&1], expected);
        assert_eq!(result.full_reads.exact, !invalid);
        assert_eq!(result.full_reads.observed, if invalid { 6 } else { 7 });
    }
    Ok(())
}

#[test]
fn cancellation_before_admission_starts_no_reads() -> Result<(), VerifyError> {
    let a = item(1, 2);
    let b = item(2, 2);
    let mut port = FakePort::new(&[(a.clone(), b"xx".to_vec()), (b.clone(), b"xx".to_vec())]);
    let (sample, full) = hashers();
    let result = verification::verify(&[a, b], &mut port, strategies(&sample, &full), || true)?;
    assert!(result.cancelled);
    assert_eq!(result.unjudged, vec![1, 2]);
    assert!(result.interrupted.is_empty());
    assert!(port.opens.is_empty());
    Ok(())
}

#[test]
fn cancellation_retains_interrupted_aliases_and_unjudged_candidates() -> Result<(), VerifyError> {
    let a = item(1, 2);
    let mut alias = a.clone();
    alias.target.item_no = 3;
    let b = item(2, 2);
    let flag = Rc::new(Cell::new(false));
    let mut port = FakePort::new(&[
        (a.clone(), b"xx".to_vec()),
        (b.clone(), b"xx".to_vec()),
        (alias.clone(), b"xx".to_vec()),
    ]);
    port.cancel_on_read = Some(flag.clone());
    let result = verification::verify(&[a, b, alias], &mut port, Strategies::blake3_v1(), || {
        flag.get()
    })?;
    assert!(result.cancelled);
    assert_eq!(port.opens, vec![1]);
    assert_eq!(result.interrupted, vec![1, 3]);
    assert_eq!(result.unjudged, vec![1, 2, 3]);
    assert_eq!(result.states[&1], State::Candidate);
    assert_eq!(result.states[&3], State::Candidate);
    Ok(())
}

#[test]
fn cancellation_keeps_completed_group_and_marks_later_candidates() -> Result<(), VerifyError> {
    let a = item(1, 4);
    let b = item(2, 4);
    let c = item(3, 2);
    let d = item(4, 2);
    let mut port = FakePort::new(&[
        (a.clone(), b"aaaa".to_vec()),
        (b.clone(), b"aaaa".to_vec()),
        (c.clone(), b"bb".to_vec()),
        (d.clone(), b"bb".to_vec()),
    ]);
    let calls = Cell::new(0);
    let result = verification::verify(&[a, b, c, d], &mut port, Strategies::blake3_v1(), || {
        calls.set(calls.get() + 1);
        calls.get() == 6
    })?;
    assert!(result.cancelled);
    assert_eq!(result.groups.len(), 1);
    assert_eq!(result.states[&1], State::Verified);
    assert_eq!(result.states[&2], State::Verified);
    assert_eq!(result.unjudged, vec![3, 4]);
    assert!(result.interrupted.is_empty());
    assert_eq!(port.opens, vec![1, 2]);
    Ok(())
}

#[test]
fn cancellation_after_failed_open_marks_attempt_without_final_disposition()
-> Result<(), VerifyError> {
    let a = item(1, 2);
    let b = item(2, 2);
    let flag = Rc::new(Cell::new(false));
    let mut port = FakePort::new(&[(a.clone(), b"xx".to_vec()), (b.clone(), b"xx".to_vec())]);
    port.cancel_on_open_failure = Some(flag.clone());
    let result = verification::verify(&[a, b], &mut port, Strategies::blake3_v1(), || flag.get())?;
    assert!(result.cancelled);
    assert_eq!(port.opens, vec![1]);
    assert!(port.reads.is_empty());
    assert_eq!(result.full_reads.observed, 0);
    assert!(result.full_reads.exact);
    assert_eq!(result.states[&1], State::Candidate);
    assert_eq!(result.interrupted, vec![1]);
    assert_eq!(result.unjudged, vec![1, 2]);
    Ok(())
}
