//! Capacity observations tied to the collector's verified directory handle.
use super::{WindowsDirectory, error, final_path, info, raw, scope};
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    volume::VolumeCapacity,
};
use std::mem::size_of;
use windows::{
    Wdk::{
        Storage::FileSystem::{FileFsFullSizeInformationEx, NtQueryVolumeInformationFile},
        System::SystemServices::FILE_FS_FULL_SIZE_INFORMATION_EX,
    },
    Win32::{
        Foundation::RtlNtStatusToDosError, Storage::FileSystem::VOLUME_NAME_GUID,
        System::IO::IO_STATUS_BLOCK,
    },
};

/// OS-reported capacity for one verified volume, observed through its open root.
///
/// `capacity` is quota-aware for the calling thread. `actual_free_bytes` is
/// volume-wide and must not be subtracted from a quota-limited total. These
/// numbers do not establish file extents or unique physical allocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsVolumeSpace {
    /// Volume GUID identity in UTF-16LE, without a trailing backslash.
    pub volume_id: Vec<u8>,
    /// Total and available capacity for the calling thread.
    pub capacity: VolumeCapacity,
    /// Total bytes on the volume, irrespective of caller quota.
    pub actual_total_bytes: u64,
    /// Total free bytes reported for the volume, irrespective of caller quota.
    pub actual_free_bytes: u64,
}

fn changed() -> PortError {
    PortError {
        kind: PortErrorKind::IdentityChanged,
        reason_key: "scan.disconnected".into(),
        os_code: None,
    }
}

fn checked<T>(
    expected: &[u8],
    identity: impl Fn() -> Result<Vec<u8>, PortError>,
    read: impl FnOnce() -> Result<T, PortError>,
) -> Result<T, PortError> {
    if identity()? != expected {
        return Err(changed());
    }
    let result = read();
    if identity()? != expected {
        return Err(changed());
    }
    result
}

fn amount(units: u64, cluster: u64) -> Result<u64, PortError> {
    units.checked_mul(cluster).ok_or_else(super::invalid)
}

fn read(directory: &WindowsDirectory) -> Result<WindowsVolumeSpace, PortError> {
    let mut value = FILE_FS_FULL_SIZE_INFORMATION_EX::default();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: this synchronous handle is live; the aligned output and status
    // structures are writable for their full lengths throughout the call.
    let result = unsafe {
        NtQueryVolumeInformationFile(
            raw(&directory.handle),
            &mut status,
            std::ptr::addr_of_mut!(value).cast(),
            size_of::<FILE_FS_FULL_SIZE_INFORMATION_EX>() as u32,
            FileFsFullSizeInformationEx,
        )
    };
    if result.0 < 0 {
        // SAFETY: this conversion neither dereferences memory nor performs I/O.
        return Err(error(unsafe { RtlNtStatusToDosError(result) } as i32));
    }
    if status.Information < size_of::<FILE_FS_FULL_SIZE_INFORMATION_EX>() {
        return Err(super::invalid());
    }
    let cluster = u64::from(value.SectorsPerAllocationUnit)
        .checked_mul(u64::from(value.BytesPerSector))
        .filter(|&n| n > 0)
        .ok_or_else(super::invalid)?;
    Ok(WindowsVolumeSpace {
        volume_id: directory.volume_guid.to_vec(),
        capacity: VolumeCapacity {
            total_bytes: amount(value.CallerTotalAllocationUnits, cluster)?,
            available_bytes: amount(value.CallerAvailableAllocationUnits, cluster)?,
        },
        actual_total_bytes: amount(value.ActualTotalAllocationUnits, cluster)?,
        actual_free_bytes: amount(value.ActualAvailableAllocationUnits, cluster)?,
    })
}

impl WindowsDirectory {
    /// Reads space through this verified directory, requiring the reviewed volume ID.
    ///
    /// Call separately at the beginning and end and retain both `Result`s. The
    /// caller records observation times; failure must remain an unavailable report.
    /// A different or unavailable identity prevents or invalidates the observation.
    pub fn volume_space(&self, expected_volume: &[u8]) -> Result<WindowsVolumeSpace, PortError> {
        checked(
            expected_volume,
            || {
                let details = info(&self.handle)?;
                if u64::from(details.dwVolumeSerialNumber) != self.volume {
                    return Err(changed());
                }
                let guid = scope::volume_name(&final_path(&self.handle, VOLUME_NAME_GUID)?)?;
                if guid != self.volume_guid.as_ref() {
                    return Err(changed());
                }
                Ok(guid)
            },
            || read(self),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[test]
    fn checks_identity_before_read_and_discards_after_rebind_or_failure() {
        let calls = Cell::new(0);
        let reads = Cell::new(0);
        let before_wrong = checked(
            b"expected",
            || Ok(b"other".to_vec()),
            || {
                reads.set(reads.get() + 1);
                Ok(1)
            },
        );
        assert_eq!(
            before_wrong.err().map(|e| e.kind),
            Some(PortErrorKind::IdentityChanged)
        );
        assert_eq!(reads.get(), 0);
        for succeeds in [true, false] {
            calls.set(0);
            let result = checked(
                b"expected",
                || {
                    calls.set(calls.get() + 1);
                    Ok(if calls.get() == 1 {
                        b"expected".to_vec()
                    } else {
                        b"other".to_vec()
                    })
                },
                || {
                    if succeeds {
                        Ok(1)
                    } else {
                        Err(super::super::invalid())
                    }
                },
            );
            assert_eq!(
                result.err().map(|e| e.kind),
                Some(PortErrorKind::IdentityChanged)
            );
            assert_eq!(calls.get(), 2);
        }
        let failure = super::super::invalid();
        assert_eq!(
            checked::<()>(
                b"expected",
                || Ok(b"expected".to_vec()),
                || Err(failure.clone())
            ),
            Err(failure)
        );
    }
    #[test]
    fn invalid_and_overflowing_size_are_not_reported() {
        assert!(amount(u64::MAX, 4096).is_err());
        assert_eq!(amount(2, 4096), Ok(8192));
    }
}
