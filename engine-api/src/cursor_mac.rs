//! Domain-separated authentication shared by session cursors.

pub(crate) fn mac(key: &[u8; 32], domain: &[u8], payload: &[u8]) -> blake3::Hash {
    let mut hasher = blake3::Hasher::new_keyed(key);
    hasher.update(domain);
    hasher.update(payload);
    hasher.finalize()
}
