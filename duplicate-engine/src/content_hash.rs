//! Purpose-separated BLAKE3 revision 1 for approved content observations.

use crate::verification::{
    self, Digest, HashPurpose, HashStrategy, Outcome, ReadPort, Strategies, VerificationTarget,
    VerifyError,
};

use du_core_contracts::job::{Checkpoint, JobContext};

const SAMPLE_CONTEXT: &str = "com.github.arikasalice.diskunfold.f09.blake3.v1.sample";
const FULL_CONTEXT: &str = "com.github.arikasalice.diskunfold.f09.blake3.v1.full";

struct Blake3V1;

const REGISTERED: &[&(dyn HashStrategy + Sync)] = &[&Blake3V1];

/// A content hash selected from the product registry before reading any file.
pub struct RegisteredHash(&'static (dyn HashStrategy + Sync));

/// Verification performed with a registry-selected strategy for both hash purposes.
pub struct RegisteredOutcome(Outcome);

impl RegisteredOutcome {
    pub(crate) fn prepare_unjudged(&mut self) -> Vec<u32> {
        if self.0.cancelled {
            std::mem::take(&mut self.0.unjudged)
        } else {
            self.0
                .states
                .iter()
                .filter_map(|(&item, state)| {
                    matches!(
                        state,
                        verification::State::Candidate | verification::State::SampleCandidate
                    )
                    .then_some(item)
                })
                .collect()
        }
    }
    pub(crate) fn mark_cancelled_prepared(&mut self, unjudged: Vec<u32>) {
        self.0.cancelled = true;
        self.0.unjudged = unjudged;
    }
    /// Reads the verification evidence and its exact selected strategy identity.
    pub fn evidence(&self) -> &Outcome {
        &self.0
    }
}

impl RegisteredHash {
    /// Resolves an exact revision, or the highest registered revision when omitted.
    pub fn resolve(id: &str, revision: Option<u32>) -> Result<Self, VerifyError> {
        let strategy = REGISTERED
            .iter()
            .copied()
            .filter(|strategy| {
                strategy.id() == id
                    && revision.is_none_or(|revision| strategy.revision() == revision)
            })
            .max_by_key(|strategy| strategy.revision())
            .ok_or(VerifyError::UnsupportedHash)?;
        if !strategy.collision_resistant() {
            return Err(VerifyError::UnsupportedHash);
        }
        Ok(Self(strategy))
    }

    /// Verifies selected targets while observing the job cancellation checkpoint.
    ///
    /// An in-flight file attempt reaches its boundary before cancellation returns;
    /// no new file is admitted after a checkpoint reports cancellation. This does
    /// not provide pause or content-I/O scheduling, which require runtime support.
    ///
    /// # Errors
    /// Returns `VerifyError` when targets or the selected strategy are invalid.
    pub fn verify_with_context(
        &self,
        approved: &[VerificationTarget],
        reader: &mut impl ReadPort,
        context: &dyn JobContext,
    ) -> Result<RegisteredOutcome, VerifyError> {
        self.verify(approved, reader, || {
            context.checkpoint() == Checkpoint::Cancel
        })
    }

    pub(crate) fn verify_for_scope(
        &self,
        approved: &[VerificationTarget],
        reader: &mut impl ReadPort,
        scope_proven: bool,
        cancelled: impl FnMut() -> bool,
    ) -> Result<RegisteredOutcome, VerifyError> {
        verification::verify_for_scope(
            approved,
            reader,
            Strategies {
                sample: self.0,
                full: self.0,
            },
            scope_proven,
            cancelled,
        )
        .map(RegisteredOutcome)
    }

    /// Verifies approved targets using the pinned strategy for sampling and confirmation.
    pub fn verify(
        &self,
        approved: &[VerificationTarget],
        reader: &mut impl ReadPort,
        cancelled: impl FnMut() -> bool,
    ) -> Result<RegisteredOutcome, VerifyError> {
        verification::verify(
            approved,
            reader,
            Strategies {
                sample: self.0,
                full: self.0,
            },
            cancelled,
        )
        .map(RegisteredOutcome)
    }
}

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
