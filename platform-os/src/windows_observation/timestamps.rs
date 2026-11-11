//! Convert last-write timestamps read from an already verified handle.
use super::raw;
use du_core_contracts::Timestamp;
use std::{fs::File, mem::size_of};
use windows::Win32::Storage::FileSystem::{
    FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx,
};

pub(super) fn modified_at(file: &File) -> Option<Timestamp> {
    let mut basic = FILE_BASIC_INFO::default();
    // SAFETY: file is live and basic is a correctly sized writable structure.
    unsafe {
        GetFileInformationByHandleEx(
            raw(file),
            FileBasicInfo,
            std::ptr::addr_of_mut!(basic).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    }
    .ok()?;
    filetime(basic.LastWriteTime)
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
    use super::filetime;
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
}
