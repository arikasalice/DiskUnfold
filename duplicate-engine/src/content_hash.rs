//! Purpose-separated BLAKE3 revision 1 for approved content observations.

use crate::verification::{Digest, HashPurpose, HashStrategy, Strategies};

const SAMPLE_CONTEXT: &str = "com.github.arikasalice.diskunfold.f09.blake3.v1.sample";
const FULL_CONTEXT: &str = "com.github.arikasalice.diskunfold.f09.blake3.v1.full";

struct Blake3V1;

impl Strategies<'static> {
    /// Selects the published default for both filtering and full-file verification.
    pub fn blake3_v1() -> Self {
        Self {
            sample: &Blake3V1,
            full: &Blake3V1,
        }
    }
}

impl HashStrategy for Blake3V1 {
    fn id(&self) -> &str {
        "blake3"
    }

    fn revision(&self) -> u32 {
        1
    }

    fn collision_resistant(&self) -> bool {
        true
    }

    fn start(&self, purpose: HashPurpose, approved_size: u64) -> Box<dyn Digest> {
        let mut hasher = match purpose {
            HashPurpose::Sample => blake3::Hasher::new_derive_key(SAMPLE_CONTEXT),
            HashPurpose::Full => blake3::Hasher::new_derive_key(FULL_CONTEXT),
        };
        if purpose == HashPurpose::Sample {
            hasher.update(&approved_size.to_le_bytes());
        }
        Box::new(hasher)
    }
}

impl Digest for blake3::Hasher {
    fn update(&mut self, bytes: &[u8]) {
        self.update(bytes);
    }

    fn finish(self: Box<Self>) -> Vec<u8> {
        self.finalize().as_bytes().to_vec()
    }
}
