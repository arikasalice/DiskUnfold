//! Whether the volume of a verified handle can share extents between files.
use super::raw;
use std::fs::File;
use windows::Win32::Storage::FileSystem::GetVolumeInformationByHandleW;

/// `FILE_SUPPORTS_BLOCK_REFCOUNTING`: the volume can share clusters between
/// files, as block cloning does.
const BLOCK_REFCOUNTING: u32 = 0x0800_0000;

/// True only for an NTFS volume that reports no block reference counting.
/// A failed query, another file system or the flag leaves sharing unknown.
pub(super) fn creates_no_shared_extents(file: &File) -> bool {
    let mut name = [0u16; 32];
    let mut flags = 0u32;
    // SAFETY: the handle, the flag pointer and the name buffer remain valid for the call.
    unsafe {
        GetVolumeInformationByHandleW(
            raw(file),
            None,
            None,
            None,
            Some(&mut flags),
            Some(&mut name),
        )
    }
    .is_ok()
        && no_shared_extents(&name, flags)
}

fn no_shared_extents(name: &[u16], flags: u32) -> bool {
    name.starts_with(&[78, 84, 70, 83, 0]) && flags & BLOCK_REFCOUNTING == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> [u16; 32] {
        let mut buffer = [0u16; 32];
        for (slot, unit) in buffer.iter_mut().zip(text.encode_utf16()) {
            *slot = unit;
        }
        buffer
    }

    #[test]
    fn only_ntfs_without_block_refcounting_shares_nothing() {
        assert!(no_shared_extents(&name("NTFS"), 0x0000_00ff));
        assert!(!no_shared_extents(&name("NTFS"), BLOCK_REFCOUNTING));
        assert!(!no_shared_extents(&name("ReFS"), 0));
        assert!(!no_shared_extents(&name("NTFSX"), 0));
        assert!(!no_shared_extents(&name(""), 0));
    }
}
