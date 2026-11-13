//! Convert last-write timestamps read from an already verified handle.
use super::{info, raw};
use du_core_contracts::{Timestamp, scan_observation::NativeModifiedTime};
use std::{fs::File, mem::size_of};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx,
};

pub(super) fn modified_at(file: &File) -> Option<Timestamp> {
    observe(file).0
}

/// Keeps native evidence only when metadata around the time query agree.
pub(super) fn matched_native(
    file: &File,
    before: &BY_HANDLE_FILE_INFORMATION,
    native: Option<NativeModifiedTime>,
) -> Option<NativeModifiedTime> {
    let native = native?;
    let after = info(file).ok()?;
    matching_native(before, &after, Some(native))
}

fn matching_native(
    before: &BY_HANDLE_FILE_INFORMATION,
    after: &BY_HANDLE_FILE_INFORMATION,
    native: Option<NativeModifiedTime>,
) -> Option<NativeModifiedTime> {
    let native = native?;
    let NativeModifiedTime::WindowsFiletime(ticks) = native;
    let last_write = |v: &BY_HANDLE_FILE_INFORMATION| {
        (u64::from(v.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(v.ftLastWriteTime.dwLowDateTime)
    };
    if before.dwVolumeSerialNumber != after.dwVolumeSerialNumber
        || before.nFileIndexHigh != after.nFileIndexHigh
        || before.nFileIndexLow != after.nFileIndexLow
        || before.nFileSizeHigh != after.nFileSizeHigh
        || before.nFileSizeLow != after.nFileSizeLow
        || before.dwFileAttributes != after.dwFileAttributes
        || last_write(before) != ticks.get()
        || last_write(after) != ticks.get()
    {
        return None;
    }
    Some(native)
}

/// Derives both forms from one query on the same verified handle.
pub(super) fn observe(file: &File) -> (Option<Timestamp>, Option<NativeModifiedTime>) {
    let mut basic = FILE_BASIC_INFO::default();
    // SAFETY: file is live and basic is a correctly sized writable structure.
    let result = unsafe {
        GetFileInformationByHandleEx(
            raw(file),
            FileBasicInfo,
            std::ptr::addr_of_mut!(basic).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    }
    .ok();
    if result.is_none() {
        return (None, None);
    }
    let native = u64::try_from(basic.LastWriteTime)
        .ok()
        .and_then(std::num::NonZeroU64::new)
        .map(NativeModifiedTime::WindowsFiletime);
    (filetime(basic.LastWriteTime), native)
}

fn filetime(ticks: i64) -> Option<Timestamp> {
    if ticks <= 0 {
        return None;
    }
    let millis = (ticks - 116_444_736_000_000_000i64).div_euclid(10_000);
    Timestamp::from_unix_millis(millis)
}

#[cfg(test)]
mod tests {
    use super::{filetime, matching_native};
    use du_core_contracts::scan_observation::NativeModifiedTime;
    use std::num::NonZeroU64;
    use windows::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION;
    #[test]
    fn floor_and_unavailable_boundaries() {
        let epoch = 116_444_736_000_000_000;
        assert_eq!(
            filetime(epoch - 1)
                .map(|t| t.as_str().to_owned())
                .as_deref(),
            Some("1969-12-31T23:59:59.999Z")
        );
        assert_eq!(
            filetime(epoch + 9_999)
                .map(|t| t.as_str().to_owned())
                .as_deref(),
            Some("1970-01-01T00:00:00.000Z")
        );
        assert!(filetime(0).is_none());
        assert!(filetime(-1).is_none());
        assert!(filetime(i64::MAX).is_none());
    }

    #[test]
    fn millisecond_display_cannot_identify_a_native_filetime() {
        let epoch = 116_444_736_000_000_000;
        let first = filetime(epoch + 1);
        let last = filetime(epoch + 9_999);
        assert!(first.is_some());
        assert_eq!(first, last);
        assert_ne!(last, filetime(epoch + 10_000));
        assert_ne!(epoch + 1, epoch + 9_999);
    }

    #[test]
    fn native_evidence_requires_matching_handle_observations() {
        let before = BY_HANDLE_FILE_INFORMATION {
            nFileIndexLow: 7,
            nFileSizeLow: 12,
            ftLastWriteTime: windows::Win32::Foundation::FILETIME {
                dwLowDateTime: 42,
                ..Default::default()
            },
            ..Default::default()
        };
        let native = NonZeroU64::new(42).map(NativeModifiedTime::WindowsFiletime);
        assert_eq!(matching_native(&before, &before, native), native);
        assert_eq!(matching_native(&before, &before, None), None);
        let mut changed = before;
        changed.nFileSizeLow += 1;
        assert_eq!(matching_native(&before, &changed, native), None);
        changed = before;
        changed.nFileIndexLow += 1;
        assert_eq!(matching_native(&before, &changed, native), None);
        changed = before;
        changed.ftLastWriteTime.dwLowDateTime += 1;
        assert_eq!(matching_native(&before, &changed, native), None);
        let other = NonZeroU64::new(43).map(NativeModifiedTime::WindowsFiletime);
        assert_eq!(matching_native(&before, &before, other), None);
    }
}
