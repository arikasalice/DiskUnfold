//! Windows volume-device observations.
use super::{
    DeviceKind, DeviceProbe, OS_DEVICE_MULTIPLE_DISKS, OS_DEVICE_NETWORK, OS_QUERY_FAILED,
    OS_UNSUPPORTED,
};
use std::{
    ffi::c_void,
    fs::File,
    mem::{offset_of, size_of},
    os::windows::io::{AsRawHandle, FromRawHandle},
    ptr,
};
use windows::{
    Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, GetDriveTypeW, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, OPEN_EXISTING,
        },
        System::{
            IO::DeviceIoControl,
            Ioctl::{
                DEVICE_SEEK_PENALTY_DESCRIPTOR, DISK_EXTENT, IOCTL_STORAGE_QUERY_PROPERTY,
                PropertyStandardQuery, STORAGE_PROPERTY_QUERY, StorageDeviceSeekPenaltyProperty,
                VOLUME_DISK_EXTENTS,
            },
            WindowsProgramming::DRIVE_REMOTE,
        },
    },
    core::PCWSTR,
};

const ERROR_MORE_DATA: u32 = 234;
const MAX_EXTENTS_BUFFER_BYTES: usize = 1024 * 1024;

pub(crate) fn probe_volume_guid_bytes(guid_bytes: &[u8]) -> DeviceProbe {
    let (pairs, tail) = guid_bytes.as_chunks::<2>();
    if !tail.is_empty() {
        return probe(DeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
    }
    let guid: Vec<u16> = pairs.iter().copied().map(u16::from_le_bytes).collect();
    probe_volume_guid(&guid)
}

/// Probes the volume named by a NUL-free volume GUID path, such as `\\?\Volume{...}\`.
pub fn probe_volume_guid(guid: &[u16]) -> DeviceProbe {
    if guid.is_empty() || guid.contains(&0) {
        return probe(DeviceKind::Unknown, None, Some(OS_UNSUPPORTED));
    }

    let mut volume_path = guid.to_vec();
    if volume_path.last() != Some(&(b'\\' as u16)) {
        volume_path.push(b'\\' as u16);
    }
    volume_path.push(0);
    // SAFETY: volume_path is NUL-terminated and remains alive for this call.
    if unsafe { GetDriveTypeW(PCWSTR(volume_path.as_ptr())) } == DRIVE_REMOTE {
        return probe(DeviceKind::Unknown, None, Some(OS_DEVICE_NETWORK));
    }

    volume_path.pop();
    while volume_path.last() == Some(&(b'\\' as u16)) {
        volume_path.pop();
    }
    if volume_path.is_empty() {
        return probe(DeviceKind::Unknown, None, Some(OS_UNSUPPORTED));
    }
    volume_path.push(0);
    // SAFETY: the NUL-terminated name targets the volume device object, never a user file.
    let handle = match unsafe {
        CreateFileW(
            PCWSTR(volume_path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    } {
        Ok(handle) => handle,
        Err(_) => return probe(DeviceKind::Unknown, None, Some(OS_QUERY_FAILED)),
    };
    // SAFETY: CreateFileW returned an owned, live handle which File closes on drop.
    let file = unsafe { File::from_raw_handle(handle.0) };
    let kind = query_kind(&file);
    let extents = query_extents(&file);
    match extents {
        Ok(Extents::One(disk_number)) => {
            let disk_key = format!("win:disk:{disk_number}").into_bytes();
            let reason = (kind == DeviceKind::Unknown).then_some(OS_QUERY_FAILED);
            probe(kind, Some(disk_key), reason)
        }
        Ok(Extents::Multiple) => probe(DeviceKind::Unknown, None, Some(OS_DEVICE_MULTIPLE_DISKS)),
        Err(()) => probe(kind, None, Some(OS_QUERY_FAILED)),
    }
}

fn query_kind(file: &File) -> DeviceKind {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        ..Default::default()
    };
    let mut descriptor = DEVICE_SEEK_PENALTY_DESCRIPTOR::default();
    let mut returned = 0u32;
    // SAFETY: both typed buffers remain live and writable for their declared byte lengths.
    let result = unsafe {
        DeviceIoControl(
            HANDLE(file.as_raw_handle()),
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&query as *const STORAGE_PROPERTY_QUERY).cast::<c_void>()),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some((&mut descriptor as *mut DEVICE_SEEK_PENALTY_DESCRIPTOR).cast::<c_void>()),
            size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
            Some(&mut returned),
            None,
        )
    };
    if result.is_err() || returned < size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32 {
        DeviceKind::Unknown
    } else if descriptor.IncursSeekPenalty {
        DeviceKind::Hdd
    } else {
        DeviceKind::Ssd
    }
}

enum Extents {
    One(u32),
    Multiple,
}

fn query_extents(file: &File) -> Result<Extents, ()> {
    let mut capacity = 4096usize;
    loop {
        let mut buffer = vec![0u64; capacity.div_ceil(size_of::<u64>())];
        let mut returned = 0u32;
        // SAFETY: the aligned output buffer is writable for capacity bytes and the handle is live.
        let result = unsafe {
            DeviceIoControl(
                HANDLE(file.as_raw_handle()),
                IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
                None,
                0,
                Some(buffer.as_mut_ptr().cast::<c_void>()),
                capacity as u32,
                Some(&mut returned),
                None,
            )
        };
        match result {
            Ok(()) => return parse_extents(&buffer, returned as usize),
            Err(error)
                if (error.code().0 as u32 & 0xffff) == ERROR_MORE_DATA
                    && capacity < MAX_EXTENTS_BUFFER_BYTES =>
            {
                capacity = capacity
                    .checked_mul(2)
                    .ok_or(())?
                    .min(MAX_EXTENTS_BUFFER_BYTES);
            }
            Err(_) => return Err(()),
        }
    }
}

fn parse_extents(buffer: &[u64], returned: usize) -> Result<Extents, ()> {
    let base = buffer.as_ptr().cast::<u8>();
    let extent_offset = offset_of!(VOLUME_DISK_EXTENTS, Extents);
    if returned < size_of::<u32>() {
        return Err(());
    }
    // SAFETY: the returned buffer contains at least the leading extent count.
    let count = unsafe { ptr::read_unaligned(base.cast::<u32>()) } as usize;
    let required = extent_offset
        .checked_add(count.checked_mul(size_of::<DISK_EXTENT>()).ok_or(())?)
        .ok_or(())?;
    if count == 0 || returned < required {
        return Err(());
    }

    let disk_offset = offset_of!(DISK_EXTENT, DiskNumber);
    let mut only_disk = None;
    for index in 0..count {
        let offset = extent_offset
            .checked_add(index.checked_mul(size_of::<DISK_EXTENT>()).ok_or(())?)
            .and_then(|offset| offset.checked_add(disk_offset))
            .ok_or(())?;
        // SAFETY: required verifies that every extent's DiskNumber fits in returned bytes.
        let disk_number = unsafe { ptr::read_unaligned(base.add(offset).cast::<u32>()) };
        if only_disk.is_some_and(|previous| previous != disk_number) {
            return Ok(Extents::Multiple);
        }
        only_disk = Some(disk_number);
    }
    only_disk.map(Extents::One).ok_or(())
}

fn probe(
    kind: DeviceKind,
    disk_key: Option<Vec<u8>>,
    reason_key: Option<&'static str>,
) -> DeviceProbe {
    DeviceProbe {
        kind,
        disk_key,
        reason_key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{error::Error, io, os::windows::ffi::OsStrExt};
    use windows::Win32::Storage::FileSystem::{
        GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    };

    #[test]
    fn windows_temp_volume_probe_smoke() -> Result<(), Box<dyn Error>> {
        let temp = std::env::temp_dir();
        let path: Vec<u16> = temp.as_os_str().encode_wide().chain([0]).collect();
        let mut mount_point = [0u16; 260];
        // SAFETY: both strings are NUL-terminated and mount_point is writable.
        unsafe { GetVolumePathNameW(PCWSTR(path.as_ptr()), &mut mount_point) }?;
        let mut volume_guid = [0u16; 64];
        // SAFETY: mount_point is terminated and volume_guid is writable.
        unsafe {
            GetVolumeNameForVolumeMountPointW(PCWSTR(mount_point.as_ptr()), &mut volume_guid)
        }?;
        let end = volume_guid
            .iter()
            .position(|unit| *unit == 0)
            .ok_or_else(|| io::Error::other("volume GUID was not terminated"))?;
        let result = probe_volume_guid(&volume_guid[..end]);
        eprintln!("Windows temp-volume device probe: {result:?}");
        if result
            .disk_key
            .as_deref()
            .is_some_and(|key| !key.starts_with(b"win:disk:"))
        {
            return Err(io::Error::other("Windows disk key has an unexpected prefix").into());
        }
        Ok(())
    }

    #[test]
    fn parses_one_extent_per_distinct_disk() -> Result<(), Box<dyn Error>> {
        let bytes = size_of::<VOLUME_DISK_EXTENTS>() + size_of::<DISK_EXTENT>();
        let mut buffer = vec![0u64; bytes.div_ceil(size_of::<u64>())];
        let base = buffer.as_mut_ptr().cast::<u8>();
        let offset = offset_of!(VOLUME_DISK_EXTENTS, Extents);
        // SAFETY: the buffer has space for two extents and all writes are aligned-safe.
        unsafe {
            ptr::write_unaligned(base.cast::<u32>(), 2);
            ptr::write_unaligned(base.add(offset).cast::<u32>(), 7);
            ptr::write_unaligned(base.add(offset + size_of::<DISK_EXTENT>()).cast::<u32>(), 7);
        }
        if !matches!(parse_extents(&buffer, bytes), Ok(Extents::One(7))) {
            return Err(io::Error::other("same-disk extents were not deduplicated").into());
        }
        // SAFETY: the second extent DiskNumber is within the allocated buffer.
        unsafe {
            ptr::write_unaligned(base.add(offset + size_of::<DISK_EXTENT>()).cast::<u32>(), 8);
        }
        if !matches!(parse_extents(&buffer, bytes), Ok(Extents::Multiple)) {
            return Err(io::Error::other("multi-disk extents were not detected").into());
        }
        Ok(())
    }
}
