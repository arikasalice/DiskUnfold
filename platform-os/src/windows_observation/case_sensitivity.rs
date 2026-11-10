//! Per-directory case flag observations, not a filesystem name comparator.
use super::{WindowsDirectory, identity, info, invalid, raw, recall, verified_directory, wide};
use du_core_contracts::ports::{PortError, PortErrorKind};
use du_core_contracts::scan_observation::{ObservedEntry, ObservedKind};
use std::collections::VecDeque;
use std::mem::size_of;
use windows::{
    Wdk::Storage::FileSystem::{
        FILE_CASE_SENSITIVE_INFORMATION, FileCaseSensitiveInformation, NtQueryInformationFile,
    },
    Win32::{
        Foundation::RtlNtStatusToDosError,
        Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT},
        System::IO::IO_STATUS_BLOCK,
    },
};

// FILE_CS_FLAG_CASE_SENSITIVE_DIR, documented with FILE_CASE_SENSITIVE_INFORMATION.
const CASE_SENSITIVE_DIRECTORY: u32 = 1;

/// The flag reported for one directory at the instant it was queried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryCaseMode {
    /// The directory reports case-sensitive name lookup.
    Sensitive,
    /// The directory does not report the case-sensitive flag.
    Insensitive,
}

/// A point-in-time flag tied to the identity of the queried directory handle.
///
/// This is not a volume-wide rule, name equivalence test, ordering key, or
/// authorization to traverse a directory. The flag may change after observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectoryCaseObservation {
    pub mode: DirectoryCaseMode,
    pub volume_serial: u64,
    pub file_index: u64,
}

fn unsupported(code: Option<i32>) -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "os.query_failed".into(),
        os_code: code,
    }
}

fn mode(flags: u32) -> Result<DirectoryCaseMode, PortError> {
    match flags {
        0 => Ok(DirectoryCaseMode::Insensitive),
        CASE_SENSITIVE_DIRECTORY => Ok(DirectoryCaseMode::Sensitive),
        _ => Err(unsupported(None)),
    }
}

impl WindowsDirectory {
    /// Opens an enumerated child using the same identity checks as the scan port.
    ///
    /// The observation is tied to the returned open handle. A failed flag query
    /// is retained as an error and does not prevent using the directory for
    /// collection; it must never be interpreted as case-insensitive. This
    /// Windows-only method is not yet exposed through `AnchoredDirectory`.
    pub fn open_child_with_case(
        &self,
        entry: &ObservedEntry,
    ) -> Result<(Self, Result<DirectoryCaseObservation, PortError>), PortError> {
        self.open_child_with_case_using(entry, Self::case_sensitivity)
    }

    fn open_child_with_case_using(
        &self,
        entry: &ObservedEntry,
        observe: impl FnOnce(&Self) -> Result<DirectoryCaseObservation, PortError>,
    ) -> Result<(Self, Result<DirectoryCaseObservation, PortError>), PortError> {
        let directory = self.open_child_directory(entry)?;
        let observation = observe(&directory);
        Ok((directory, observation))
    }

    pub(super) fn open_child_directory(&self, entry: &ObservedEntry) -> Result<Self, PortError> {
        let meta = entry.metadata.as_ref().map_err(|e| e.clone())?;
        if meta.kind != ObservedKind::Directory
            || meta.device != self.volume
            || meta.inode == 0
            || entry.enumerated_inode != meta.inode
        {
            return Err(invalid());
        }
        let name = wide(&entry.name)?;
        let handle = verified_directory(&self.handle, &name, self.volume, Some(meta.inode), false)?;
        Ok(Self {
            handle,
            volume: self.volume,
            volume_guid: self.volume_guid.clone(),
            restart: true,
            pending: VecDeque::new(),
            filtered: None,
            filter_failed: false,
            finished: false,
            ancestors: None,
        })
    }

    /// Queries this verified handle without opening or enumerating a child.
    ///
    /// Failure and unrecognized flag bits are returned as unsupported; callers
    /// must not infer a mode from a volume feature or from an ancestor's flag.
    pub fn case_sensitivity(&self) -> Result<DirectoryCaseObservation, PortError> {
        let before = info(&self.handle)?;
        recall::check_handle_attributes(before.dwFileAttributes)?;
        if before.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
            || before.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || identity(&before) == 0
        {
            return Err(unsupported(None));
        }
        let mut data = FILE_CASE_SENSITIVE_INFORMATION::default();
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: the directory owns a live synchronous handle, and both output
        // objects are writable and remain alive for this synchronous query.
        let result = unsafe {
            NtQueryInformationFile(
                raw(&self.handle),
                &mut status,
                std::ptr::addr_of_mut!(data).cast(),
                size_of::<FILE_CASE_SENSITIVE_INFORMATION>() as u32,
                FileCaseSensitiveInformation,
            )
        };
        if result.0 != 0 {
            // SAFETY: this function only converts an NTSTATUS value.
            return Err(unsupported(Some(
                unsafe { RtlNtStatusToDosError(result) } as i32
            )));
        }
        if status.Information != size_of::<FILE_CASE_SENSITIVE_INFORMATION>() {
            return Err(unsupported(None));
        }
        let mode = mode(data.Flags)?;
        let after = info(&self.handle)?;
        recall::check_handle_attributes(after.dwFileAttributes)?;
        if identity(&after) != identity(&before)
            || after.dwVolumeSerialNumber != before.dwVolumeSerialNumber
            || after.dwFileAttributes != before.dwFileAttributes
        {
            return Err(PortError {
                kind: PortErrorKind::IdentityChanged,
                reason_key: "os.query_failed".into(),
                os_code: None,
            });
        }
        Ok(DirectoryCaseObservation {
            mode,
            volume_serial: u64::from(before.dwVolumeSerialNumber),
            file_index: identity(&before),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_flag_bits_are_not_interpreted() {
        assert_eq!(
            mode(0).map(|v| v == DirectoryCaseMode::Insensitive),
            Ok(true)
        );
        assert_eq!(mode(1).map(|v| v == DirectoryCaseMode::Sensitive), Ok(true));
        assert!(mode(2).is_err());
        assert!(mode(3).is_err());
    }

    #[test]
    fn query_failure_is_preserved_with_usable_child() -> Result<(), Box<dyn std::error::Error>> {
        use du_core_contracts::scan_observation::DirectoryObservations;
        let temp = tempfile::tempdir()?;
        std::fs::create_dir(temp.path().join("child"))?;
        let mut parent = super::super::open_verified(temp.path()).map_err(|e| format!("{e:?}"))?;
        let entry = parent
            .next_batch()
            .map_err(|e| format!("{e:?}"))?
            .into_iter()
            .next()
            .ok_or("missing child")?;
        let failure = unsupported(Some(1));
        let (child, observed) = parent
            .open_child_with_case_using(&entry, |_| Err(failure.clone()))
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(observed, Err(failure));
        assert_ne!(child.identity().map_err(|e| format!("{e:?}"))?.file_id, 0);
        Ok(())
    }
}
