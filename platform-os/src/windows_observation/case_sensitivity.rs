//! Per-directory case flag observations, not a filesystem name comparator.
use super::{WindowsDirectory, identity, info, invalid, raw, recall, verified_directory, wide};
use du_core_contracts::ports::{PortError, PortErrorKind};
use du_core_contracts::scan_observation::{
    DirectoryCaseEvidence, DirectoryCaseState, ObservedEntry, ObservedKind,
};
use std::collections::VecDeque;
use std::mem::size_of;
use std::sync::Arc;
use windows::{
    Wdk::Storage::FileSystem::{
        FILE_CASE_SENSITIVE_INFORMATION, FileCaseSensitiveInformation, NtQueryInformationFile,
    },
    Win32::{
        Foundation::{
            RtlNtStatusToDosError, STATUS_INVALID_DEVICE_REQUEST, STATUS_INVALID_INFO_CLASS,
            STATUS_NOT_SUPPORTED,
        },
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

fn query_error(status: i32) -> PortError {
    // These statuses mean the information class or operation is unsupported.
    let unsupported_status = [
        STATUS_INVALID_INFO_CLASS,
        STATUS_INVALID_DEVICE_REQUEST,
        STATUS_NOT_SUPPORTED,
    ]
    .iter()
    .any(|candidate| candidate.0 == status);
    PortError {
        kind: if unsupported_status {
            PortErrorKind::Unsupported
        } else {
            PortErrorKind::Io
        },
        reason_key: "os.query_failed".into(),
        // SAFETY: this function only converts an NTSTATUS value.
        os_code: Some(
            unsafe { RtlNtStatusToDosError(windows::Win32::Foundation::NTSTATUS(status)) } as i32,
        ),
    }
}

/// Maps a completed handle observation without inventing a path comparison key.
pub(super) fn evidence(
    result: Result<DirectoryCaseObservation, PortError>,
    observed_at: Option<du_core_contracts::Timestamp>,
) -> DirectoryCaseEvidence {
    let (state, reason_key) = match result {
        Ok(value) => (
            match value.mode {
                DirectoryCaseMode::Sensitive => DirectoryCaseState::Sensitive,
                DirectoryCaseMode::Insensitive => DirectoryCaseState::Insensitive,
            },
            None,
        ),
        Err(error) => (
            if error.kind == PortErrorKind::Unsupported {
                DirectoryCaseState::Unsupported
            } else {
                DirectoryCaseState::Failed
            },
            Some(error.reason_key),
        ),
    };
    DirectoryCaseEvidence {
        state,
        observed_at,
        reason_key,
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
            handle: Arc::new(handle),
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
    /// Explicitly unsupported queries and unrecognized flag bits are unsupported;
    /// other query or identity failures remain errors. Callers must not infer a
    /// mode from a volume feature or from an ancestor's flag.
    pub fn case_sensitivity(&self) -> Result<DirectoryCaseObservation, PortError> {
        self.case_sensitivity_with_clock(&|| None).0
    }

    /// Observes the flag once and timestamps only an attempted native query.
    pub(super) fn case_sensitivity_with_clock(
        &self,
        clock: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> (
        Result<DirectoryCaseObservation, PortError>,
        Option<du_core_contracts::Timestamp>,
    ) {
        let mut observed_at = None;
        let result = self.query_case(&mut || {
            observed_at = clock();
        });
        (result, observed_at)
    }

    fn query_case(
        &self,
        after_query: &mut dyn FnMut(),
    ) -> Result<DirectoryCaseObservation, PortError> {
        let before = info(&self.handle)?;
        recall::check_handle_attributes(before.dwFileAttributes).map_err(preflight_failed)?;
        if before.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
            || before.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || identity(&before) == 0
        {
            return Err(PortError {
                kind: PortErrorKind::IdentityChanged,
                reason_key: "os.query_failed".into(),
                os_code: None,
            });
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
        after_query();
        if result.0 != 0 {
            return Err(query_error(result.0));
        }
        if status.Information != size_of::<FILE_CASE_SENSITIVE_INFORMATION>() {
            return Err(PortError {
                kind: PortErrorKind::Io,
                reason_key: "os.query_failed".into(),
                os_code: None,
            });
        }
        let mode = mode(data.Flags)?;
        let after = info(&self.handle)?;
        recall::check_handle_attributes(after.dwFileAttributes).map_err(preflight_failed)?;
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

fn preflight_failed(mut error: PortError) -> PortError {
    error.kind = PortErrorKind::Io;
    error
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
    fn query_status_and_identity_failures_remain_distinct() {
        use windows::Win32::Foundation::STATUS_ACCESS_DENIED;
        for (mode, state) in [
            (DirectoryCaseMode::Sensitive, DirectoryCaseState::Sensitive),
            (
                DirectoryCaseMode::Insensitive,
                DirectoryCaseState::Insensitive,
            ),
        ] {
            let value = evidence(
                Ok(DirectoryCaseObservation {
                    mode,
                    volume_serial: 1,
                    file_index: 2,
                }),
                None,
            );
            assert_eq!(value.state, state);
            assert_eq!(value.reason_key, None);
        }
        for status in [
            STATUS_INVALID_INFO_CLASS,
            STATUS_INVALID_DEVICE_REQUEST,
            STATUS_NOT_SUPPORTED,
        ] {
            assert_eq!(query_error(status.0).kind, PortErrorKind::Unsupported);
        }
        assert_eq!(query_error(STATUS_ACCESS_DENIED.0).kind, PortErrorKind::Io);
        for kind in [PortErrorKind::Io, PortErrorKind::IdentityChanged] {
            let value = evidence(
                Err(PortError {
                    kind,
                    reason_key: "os.query_failed".into(),
                    os_code: None,
                }),
                None,
            );
            assert_eq!(value.state, DirectoryCaseState::Failed);
            assert_eq!(value.reason_key.as_deref(), Some("os.query_failed"));
        }
        assert_eq!(
            evidence(Err(unsupported(None)), None).state,
            DirectoryCaseState::Unsupported
        );
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
