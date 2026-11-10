//! Conservative pre-open checks for offline and virtualized Windows entries.
use super::{BUFFER_SIZE, error, invalid, raw};
use du_core_contracts::ports::{PortError, PortErrorKind};
use std::{
    fs::File,
    mem::{offset_of, size_of},
};
use windows::{
    Wdk::Storage::FileSystem::{
        FILE_ID_BOTH_DIR_INFORMATION, FileIdBothDirectoryInformation, NtQueryDirectoryFile,
    },
    Win32::{
        Foundation::{RtlNtStatusToDosError, UNICODE_STRING},
        Storage::FileSystem::{
            FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
            FILE_ATTRIBUTE_RECALL_ON_OPEN,
        },
        System::IO::IO_STATUS_BLOCK,
    },
    core::PWSTR,
};

fn unsafe_observation() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "os.unsupported".into(),
        os_code: None,
    }
}
/// Rejects known recall indicators from a directory enumeration record.
pub(super) fn check_enumerated_attributes(attributes: u32) -> Result<(), PortError> {
    let mask = (FILE_ATTRIBUTE_RECALL_ON_OPEN
        | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
        | FILE_ATTRIBUTE_OFFLINE)
        .0;
    if attributes & mask != 0 {
        Err(unsafe_observation())
    } else {
        Ok(())
    }
}
/// Checks handle attributes without interpreting the enumeration-only bit.
pub(super) fn check_handle_attributes(attributes: u32) -> Result<(), PortError> {
    let mask = (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_OFFLINE).0;
    if attributes & mask != 0 {
        Err(unsafe_observation())
    } else {
        Ok(())
    }
}
/// Ensures the supplied open operation is not called for a known-risk entry.
pub(super) fn observe_if_safe<T>(
    attributes: u32,
    open: impl FnOnce() -> Result<T, PortError>,
) -> Result<T, PortError> {
    check_enumerated_attributes(attributes)?;
    open()
}
pub(super) struct Entry {
    pub attributes: u32,
    pub inode: u64,
}

/// Finds one literal scope component on a fresh, already-open parent handle.
/// The first query fixes the search expression for this handle, so a handle
/// used here must never subsequently be used to enumerate a different name.
/// The later open must revalidate the file ID because this lookup is not atomic.
pub(super) fn lookup(parent: &File, wanted: &[u16]) -> Result<Entry, PortError> {
    // NT directory search accepts DOS wildcard tokens as well as * and ?.
    // Refuse them instead of treating a pattern result as an exact name.
    if wanted.is_empty()
        || wanted
            .iter()
            .any(|unit| matches!(*unit, 34 | 42 | 60 | 62 | 63))
    {
        return Err(invalid());
    }
    let length =
        u16::try_from(wanted.len().checked_mul(2).ok_or_else(invalid)?).map_err(|_| invalid())?;
    let name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: PWSTR(wanted.as_ptr().cast_mut()),
    };
    let mut buffer = vec![0u64; BUFFER_SIZE / size_of::<u64>()];
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: the parent is a live synchronous handle, name and output buffer
    // are valid for the call, and no callback or asynchronous event is used.
    let result = unsafe {
        NtQueryDirectoryFile(
            raw(parent),
            None,
            None,
            None,
            &mut status,
            buffer.as_mut_ptr().cast(),
            BUFFER_SIZE as u32,
            FileIdBothDirectoryInformation,
            true,
            Some(&name),
            true,
        )
    };
    if result.0 < 0 {
        // SAFETY: status conversion does not dereference pointers.
        return Err(error(unsafe { RtlNtStatusToDosError(result) } as i32));
    }
    let header = offset_of!(FILE_ID_BOTH_DIR_INFORMATION, FileName);
    if status.Information < header || status.Information > BUFFER_SIZE {
        return Err(invalid());
    }
    // SAFETY: buffer holds a complete fixed prefix, and read_unaligned permits
    // alignment-independent parsing of the OS-provided record.
    let item =
        unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<FILE_ID_BOTH_DIR_INFORMATION>()) };
    let length = usize::try_from(item.FileNameLength).map_err(|_| invalid())?;
    if length == 0
        || length % 2 != 0
        || header
            .checked_add(length)
            .is_none_or(|end| end > status.Information)
        || item.NextEntryOffset != 0
        || item.FileId == 0
    {
        return Err(invalid());
    }
    Ok(Entry {
        attributes: item.FileAttributes,
        inode: item.FileId as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::windows::ffi::OsStrExt;
    #[test]
    fn risk_flags_and_combinations_never_invoke_the_open_closure() {
        let flags = [
            FILE_ATTRIBUTE_RECALL_ON_OPEN.0,
            FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0,
            FILE_ATTRIBUTE_OFFLINE.0,
        ];
        for mask in 1..8 {
            let attributes = flags
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .fold(0, |value, (_, flag)| value | flag);
            for extra in [0, 0x10, 0x400, 0x410] {
                let calls = Cell::new(0);
                let result = observe_if_safe(attributes | extra, || {
                    calls.set(calls.get() + 1);
                    Ok(())
                });
                assert_eq!(calls.get(), 0);
                assert_eq!(
                    result.err().map(|e| e.kind),
                    Some(PortErrorKind::Unsupported)
                );
            }
        }
    }
    #[test]
    fn ordinary_and_reparse_attributes_preserve_open_path() {
        let calls = Cell::new(0);
        let result = observe_if_safe(0x400, || {
            calls.set(calls.get() + 1);
            Ok(42)
        });
        assert_eq!(result.ok(), Some(42));
        assert_eq!(calls.get(), 1);
        assert!(check_handle_attributes(FILE_ATTRIBUTE_RECALL_ON_OPEN.0).is_ok());
        assert!(check_handle_attributes(FILE_ATTRIBUTE_OFFLINE.0).is_err());
    }

    #[test]
    fn filtered_query_reuse_is_not_used_for_production() -> Result<(), Box<dyn std::error::Error>> {
        use crate::windows_observation::open_verified;
        let fixture = tempfile::tempdir()?;
        std::fs::create_dir(fixture.path().join("alpha"))?;
        std::fs::create_dir(fixture.path().join("beta"))?;
        let parent = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
        let alpha: Vec<u16> = std::ffi::OsStr::new("alpha").encode_wide().collect();
        let beta: Vec<u16> = std::ffi::OsStr::new("beta").encode_wide().collect();
        let first = lookup(&parent.handle, &alpha).map_err(|e| format!("{e:?}"))?;
        let second = lookup(&parent.handle, &beta).map_err(|e| format!("{e:?}"))?;
        let fresh = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
        let actual_beta = lookup(&fresh.handle, &beta).map_err(|e| format!("{e:?}"))?;
        assert_ne!(first.inode, actual_beta.inode);
        // Windows implementations can differ from the documented capture rule.
        // Production therefore does not reuse this handle for another query.
        assert!(second.inode == first.inode || second.inode == actual_beta.inode);
        Ok(())
    }

    #[test]
    fn rejects_wildcard_tokens_before_querying_the_handle() -> Result<(), Box<dyn std::error::Error>>
    {
        use crate::windows_observation::open_verified;
        let fixture = tempfile::tempdir()?;
        std::fs::create_dir(fixture.path().join("alpha"))?;
        let parent = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
        for token in [34, 42, 60, 62, 63] {
            assert!(lookup(&parent.handle, &[token]).is_err());
        }
        let alpha: Vec<u16> = std::ffi::OsStr::new("alpha").encode_wide().collect();
        assert!(lookup(&parent.handle, &alpha).is_ok());
        Ok(())
    }

    #[test]
    fn query_requires_directory_list_access() -> Result<(), Box<dyn std::error::Error>> {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        use windows::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_NO_RECALL, FILE_READ_ATTRIBUTES, SYNCHRONIZE,
        };
        let fixture = tempfile::tempdir()?;
        std::fs::create_dir(fixture.path().join("alpha"))?;
        let handle = OpenOptions::new()
            .access_mode((FILE_READ_ATTRIBUTES | SYNCHRONIZE).0)
            .share_mode(7)
            .custom_flags((FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_NO_RECALL).0)
            .open(fixture.path())?;
        let name: Vec<u16> = std::ffi::OsStr::new("alpha").encode_wide().collect();
        assert_eq!(
            lookup(&handle, &name).err().map(|e| e.kind),
            Some(PortErrorKind::PermissionDenied)
        );
        Ok(())
    }

    #[test]
    fn replacing_a_filtered_match_fails_the_handle_identity_check()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::windows_observation::{open_verified, verified_directory};
        let fixture = tempfile::tempdir()?;
        let original = fixture.path().join("child");
        std::fs::create_dir(&original)?;
        let parent = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
        let name: Vec<u16> = std::ffi::OsStr::new("child").encode_wide().collect();
        let observed = lookup(&parent.handle, &name).map_err(|e| format!("{e:?}"))?;
        std::fs::rename(&original, fixture.path().join("old"))?;
        std::fs::create_dir(&original)?;
        assert!(
            verified_directory(&parent.handle, &name, parent.volume, Some(observed.inode)).is_err()
        );
        Ok(())
    }

    #[test]
    fn filtered_scope_open_matches_direct_relative_open() -> Result<(), Box<dyn std::error::Error>>
    {
        use crate::windows_observation::{identity, info, open_verified, verified_directory};
        let fixture = tempfile::tempdir()?;
        std::fs::create_dir(fixture.path().join("École"))?;
        let parent = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
        let requested: Vec<u16> = std::ffi::OsStr::new("éCOLE").encode_wide().collect();
        let direct = verified_directory(&parent.handle, &requested, parent.volume, None)
            .map_err(|e| format!("{e:?}"))?;
        let expected = identity(&info(&direct).map_err(|e| format!("{e:?}"))?);
        let seen = lookup(&parent.handle, &requested).map_err(|e| format!("{e:?}"))?;
        let checked =
            verified_directory(&parent.handle, &requested, parent.volume, Some(seen.inode))
                .map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            expected,
            identity(&info(&checked).map_err(|e| format!("{e:?}"))?)
        );
        Ok(())
    }
}
