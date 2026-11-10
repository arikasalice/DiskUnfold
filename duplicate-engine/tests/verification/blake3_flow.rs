use super::{FakePort, State, Strategies, VerifyError, item, verification};

#[test]
fn blake3_v1_compares_full_bytes_after_matching_samples_and_short_reads() -> Result<(), VerifyError>
{
    const SIZE: usize = 128 * 1024 + 1;
    let a = item(1, SIZE);
    let b = item(2, SIZE);
    let c = item(3, SIZE);
    let same = vec![0x11; SIZE];
    let mut different = same.clone();
    different[SIZE / 2] ^= 0xff;
    let mut port = FakePort::new(&[
        (a.clone(), same.clone()),
        (b.clone(), same.clone()),
        (c.clone(), different),
    ]);
    port.max_read = 4093;
    let result = verification::verify(&[a, b, c], &mut port, Strategies::blake3_v1(), || false)?;
    assert_eq!(result.groups.len(), 1);
    assert_eq!(result.groups[0].files.len(), 2);
    assert_eq!(
        result.groups[0].digest,
        blake3::derive_key(
            "com.github.arikasalice.diskunfold.f09.blake3.v1.full",
            &same
        )
    );
    assert_eq!(result.states.get(&3), Some(&State::Distinct));
    Ok(())
}
