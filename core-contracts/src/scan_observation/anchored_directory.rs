//! Methods on an already opened, identity-verified directory.
use super::*;

/// A directory already opened and verified by an operating-system adapter.
/// Child opens stay relative to this handle and must reject changed identity or volume.
pub trait AnchoredDirectory: DirectoryObservations {
    /// Opens the previously observed directory entry after revalidating it.
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError>;

    /// Reads device facts from this handle without opening another user path.
    /// The default reports unsupported without I/O.
    fn device(&self) -> VolumeDevice {
        VolumeDevice::unsupported()
    }

    /// Optionally observes name lookup mode on this handle without reopening a path.
    fn observe_directory_case(
        &self,
        _clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Option<DirectoryCaseEvidence> {
        None
    }

    /// Retains read-only volume evidence without reopening a path.
    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "space.volume_report_unavailable".into(),
            os_code: None,
        })
    }

    /// Reads the last-write time from this verified handle without reopening a path.
    fn observed_modified_at(&self) -> Option<Timestamp> {
        None
    }

    /// True only when the volume cannot share extents between files. `false` means unknown.
    fn creates_no_shared_extents(&self) -> bool {
        false
    }

    /// Timed metadata read; existing adapters preserve unknown observation times.
    fn observed_modified_at_timed(
        &self,
        _clock: &dyn Fn() -> Option<Timestamp>,
    ) -> (Option<Timestamp>, Option<Timestamp>) {
        (self.observed_modified_at(), None)
    }

    /// Verified same-volume ancestors; `None` means unavailable.
    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        None
    }

    /// Identity of this handle in the `MetadataObservation` identity space.
    fn observed_identity(&self) -> Option<HandleIdentity>;
}
