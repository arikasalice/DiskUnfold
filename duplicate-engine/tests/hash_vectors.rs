use du_duplicate_engine::verification::{HashPurpose, Strategies};

#[test]
fn blake3_matches_upstream_empty_derive_key_vector() {
    // BLAKE3 1.8.3 test_vectors.json, input_len = 0, first 32 output bytes.
    let hasher = blake3::Hasher::new_derive_key("BLAKE3 2019-12-27 16:29:52 test vectors context");
    assert_eq!(
        hasher.finalize().to_hex().as_str(),
        "2cc39783c223154fea8dfb7c1b1660f2ac2dcbd1c1de8277b0b0dd39b7e50d7d"
    );
}

#[test]
fn blake3_v1_frames_sample_bytes_and_omits_size_from_full_bytes() {
    const K: usize = 1024;
    let head = [0x11; 64 * K];
    let tail = [0x44; 64 * K];
    let size = (128 * K + 1) as u64;
    let strategies = Strategies::blake3_v1();
    let mut digest = strategies.sample.start(HashPurpose::Sample, size);
    digest.update(&head);
    digest.update(&tail);
    let sample = digest.finish();
    let framed = [size.to_le_bytes().as_slice(), &head, &tail].concat();
    assert_eq!(
        sample,
        blake3::derive_key(
            "com.github.arikasalice.diskunfold.f09.blake3.v1.sample",
            &framed
        )
    );
    let full = strategies.full.start(HashPurpose::Full, 0).finish();
    assert_eq!(
        full,
        blake3::derive_key("com.github.arikasalice.diskunfold.f09.blake3.v1.full", &[])
    );
    assert_ne!(sample, full);
}
