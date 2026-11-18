//! Draft `BuildHandle` entry points for receiving correlated folder-attempt evidence.
use crate::{
    BuildError, BuildHandle,
    folder_evidence_api::{FolderAttemptError, FolderAttemptInput, FolderAttemptToken},
};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_BUILD_GENERATION: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_build_generation() -> Result<u64, BuildError> {
    NEXT_BUILD_GENERATION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| BuildError::CapacityExceeded)
}

impl BuildHandle {
    /// Registers one folder attempt and returns its build/root/path-bound token.
    ///
    /// Registration is outside the F02 event sequence. The first transported event uses sequence
    /// zero. Attempt IDs are never reusable during the build. A registration failure disables
    /// correlated admission so a lost start cannot be mistaken for a retry.
    /// A rootless legacy build from `IngestSink::open` has no verified root and returns
    /// `InvalidDirectory`; its ordinary observations remain independently usable.
    pub fn begin_folder_attempt(
        &mut self,
        root_index: u32,
        raw_parent: &[Vec<u8>],
        attempt_id: u64,
    ) -> Result<FolderAttemptToken, FolderAttemptError> {
        if self.finished {
            return Err(FolderAttemptError::Finished);
        }
        let Some(folder_node) = self.observations.find(root_index, raw_parent) else {
            self.folder_attempts.disable_admission();
            return Err(FolderAttemptError::InvalidDirectory);
        };
        if !self.valid_root(root_index)
            || self
                .observations
                .observation(folder_node)
                .is_none_or(|observation| {
                    observation.kind != crate::item_kind::ResultItemKind::Directory
                })
        {
            self.folder_attempts.disable_admission();
            return Err(FolderAttemptError::InvalidDirectory);
        }
        let root = self
            .roots
            .get(root_index as usize)
            .ok_or(FolderAttemptError::InvalidDirectory)?;
        let root_encoding = match &root.path.encoding {
            du_core_contracts::values::PathEncoding::Unix => 0,
            du_core_contracts::values::PathEncoding::WindowsUtf16le => 1,
        };
        self.folder_attempts.begin(
            root_index,
            folder_node,
            attempt_id,
            root_encoding,
            root.path.raw.as_str(),
            raw_parent,
        )
    }

    /// Records one F02 event under its event sequence, independent from child IDs.
    ///
    /// Sequence values are contiguous from zero. An exact same-sequence payload replay returns
    /// `Ok` only while the attempt is Pending or EndedUnproven; that acknowledges receipt of this
    /// event only and says nothing about attempt health. Held attempts reject replay, and any new
    /// sequence after End conflicts. The call never changes stored size observations.
    pub fn record_folder_attempt_event(
        &mut self,
        token: FolderAttemptToken,
        event_seq: u64,
        input: FolderAttemptInput,
    ) -> Result<(), FolderAttemptError> {
        if self.finished {
            return Err(FolderAttemptError::Finished);
        }
        self.folder_attempts.record(token, event_seq, input)
    }

    /// Ends one attempt after F02 has observed the adapter EOF and its own event barrier.
    ///
    /// `event_watermark_exclusive` counts all F02 events in `[0, watermark)`. The independent
    /// `child_count_exclusive` comes from the adapter End, which counts every issued child ID.
    /// Acceptance yields only `EndedUnproven`; it never proves filesystem stability or exact size.
    pub fn end_folder_attempt(
        &mut self,
        token: FolderAttemptToken,
        event_watermark_exclusive: u64,
        child_count_exclusive: u64,
    ) -> Result<(), FolderAttemptError> {
        if self.finished {
            return Err(FolderAttemptError::Finished);
        }
        self.folder_attempts.end(
            token,
            event_watermark_exclusive,
            child_count_exclusive,
            &self.observations,
        )
    }
}
