//! Draft production boundary for F02 to F03 folder-attempt evidence.
//!
//! These additions are implementation candidates and are not a published external contract.
use du_core_contracts::scan_observation::CorrelatedDirectoryEvent;

/// Opaque attempt identity minted by one [`crate::BuildHandle`].
///
/// The token binds the build generation, root, registered parent directory and attempt ID.
/// Keep it with the producing frame and use it for every event and the end barrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FolderAttemptToken {
    pub(crate) build_generation: u64,
    pub(crate) attempt_id: u64,
    pub(crate) root_index: u32,
    pub(crate) folder_node: u32,
}

/// Terminal admission outcome reported by F02 for an admitted direct child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderAttemptFinal {
    /// The item was stored. A traversed directory carries its registered child attempt token;
    /// the token links attempts but does not claim that descendant attempt has ended.
    Recorded {
        child_attempt: Option<FolderAttemptToken>,
    },
    /// Traversal intentionally stopped at a known scope boundary.
    Boundary,
    /// The item was not entered because it had already been visited.
    Revisit,
    /// F02 could not establish a terminal classification.
    Unresolved,
}

/// One ordered F02 event. The adapter's EOF marker is passed separately to `end_folder_attempt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderAttemptInput {
    /// Adapter callback, observation, or progress event. Adapter End uses the separate barrier API.
    Adapter(CorrelatedDirectoryEvent),
    /// F02's final direct-child classification, joined by child ID rather than list position.
    Final {
        child_id: u64,
        outcome: FolderAttemptFinal,
    },
    /// F02 discarded pending correlation data; this is unresolved and holds the attempt.
    PendingCleared { child_id: u64 },
    /// The F02 event transport failed after this attempt was selected.
    TransportFailure,
    /// The selected attempt was cancelled and cannot be resumed.
    Cancelled,
}

impl FolderAttemptInput {
    pub(crate) fn managed_payload_len(&self) -> Option<usize> {
        use du_core_contracts::scan_observation::CorrelatedDirectoryEvent as AdapterEvent;

        let bytes = match self {
            Self::Adapter(AdapterEvent::FilterCallback { raw_name, .. }) => raw_name.len(),
            Self::Adapter(AdapterEvent::Observation { entry, .. }) => {
                let mut bytes = entry.entry.name.len();
                match &entry.entry.metadata {
                    Err(error) => bytes = bytes.checked_add(error.reason_key.len())?,
                    Ok(metadata) => {
                        if let Err(error) = &metadata.size {
                            bytes = bytes.checked_add(error.reason_key.len())?;
                        }
                        if let Some(identity) = &metadata.space_identity {
                            bytes = bytes
                                .checked_add(identity.volume.len())?
                                .checked_add(identity.file.len())?;
                        }
                    }
                }
                bytes
            }
            _ => 0,
        };
        Some(bytes)
    }

    pub(crate) fn managed_payload_capacity(&self) -> Option<usize> {
        use du_core_contracts::scan_observation::CorrelatedDirectoryEvent as AdapterEvent;

        let bytes = match self {
            Self::Adapter(AdapterEvent::FilterCallback { raw_name, .. }) => raw_name.capacity(),
            Self::Adapter(AdapterEvent::Observation { entry, .. }) => {
                let mut bytes = entry.entry.name.capacity();
                match &entry.entry.metadata {
                    Err(error) => bytes = bytes.checked_add(error.reason_key.capacity())?,
                    Ok(metadata) => {
                        if let Err(error) = &metadata.size {
                            bytes = bytes.checked_add(error.reason_key.capacity())?;
                        }
                        if let Some(identity) = &metadata.space_identity {
                            bytes = bytes
                                .checked_add(identity.volume.capacity())?
                                .checked_add(identity.file.capacity())?;
                        }
                    }
                }
                bytes
            }
            _ => 0,
        };
        Some(bytes)
    }
}

/// Why an attempt receiver call was refused. Any conflict leaves the attempt held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderAttemptError {
    /// A managed-memory or fixed registry limit was reached.
    CapacityExceeded,
    /// The root/path does not name a stored directory in this build.
    InvalidDirectory,
    /// This build has already registered the ID, including ended attempts.
    AttemptIdReused,
    /// No start registration exists for this token.
    UnknownAttempt,
    /// The token belongs to a different BuildHandle generation.
    WrongBuild,
    /// A payload conflicts with an earlier event or terminal state.
    Conflict,
    /// The event sequence skipped a required position.
    SequenceGap,
    /// The EOF barrier or child/event watermarks do not validate.
    InvalidEnd,
    /// The BuildHandle has already been sealed or aborted.
    Finished,
}

/// Receiver state pinned into a result version. `EndedUnproven` is not an exactness claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderAttemptStatus {
    /// The attempt has no accepted EOF barrier yet.
    Pending,
    /// A conflict, loss, cancellation, or capacity failure prevents completion.
    Held,
    /// Transport and direct-child terminals validate; observation stability remains unproven.
    EndedUnproven,
}
