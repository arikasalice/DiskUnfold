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
    assert!(port.opens.is_empty());
    Ok(())
}
