//! Conservative pre-open checks for offline and virtualized Windows entries.
use super::{BUFFER_SIZE, error, invalid, raw, win_error};
use du_core_contracts::ports::{PortError, PortErrorKind};
use std::{
    fs::File,
    mem::{offset_of, size_of},
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    FILE_ID_BOTH_DIR_INFO, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
    GetFileInformationByHandleEx,
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

// Exact UTF-16 matches are accepted; ASCII-only case differences are also
// considered. Other case variants fail closed rather than guessing NTFS rules.
fn name_matches(a: &[u16], b: &[u16]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(&x, &y)| {
            x == y || (x <= 127 && y <= 127 && (x as u8).eq_ignore_ascii_case(&(y as u8)))
        })
}

/// Finds a scope component in its already-open parent before opening it.
/// Ambiguous matches or missing file IDs fail closed. Enumeration and the
/// later open are not atomic; the open must still verify the returned ID.
pub(super) fn lookup(parent: &File, wanted: &[u16]) -> Result<Entry, PortError> {
    let mut buffer = vec![0u64; BUFFER_SIZE / size_of::<u64>()];
    let mut restart = true;
    let mut found = None;
    loop {
        let class = if restart {
            FileIdBothDirectoryRestartInfo
        } else {
            FileIdBothDirectoryInfo
        };
        restart = false;
        // SAFETY: buffer is aligned and writable, and parent owns a live handle.
        let result = unsafe {
            GetFileInformationByHandleEx(
                raw(parent),
                class,
                buffer.as_mut_ptr().cast(),
                BUFFER_SIZE as u32,
            )
        };
        if let Err(e) = result {
            let failure = win_error(e);
            if failure.os_code == Some(18) {
                break;
            }
            return Err(failure);
        }
        scan_buffer(&buffer, wanted, &mut found)?;
    }
    let result: Entry = found.ok_or_else(|| error(2))?;
    if result.inode == 0 {
        return Err(invalid());
    }
    Ok(result)
}

fn scan_buffer(buffer: &[u64], wanted: &[u16], found: &mut Option<Entry>) -> Result<(), PortError> {
    let mut offset = 0usize;
    loop {
        let header = offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
        if offset
            .checked_add(size_of::<FILE_ID_BOTH_DIR_INFO>())
            .is_none_or(|end| end > BUFFER_SIZE)
        {
            return Err(invalid());
        }
        // SAFETY: the fixed structure fits in the aligned buffer; use an
        // unaligned read because subsequent offsets are supplied by the OS.
        let item = unsafe {
            std::ptr::read_unaligned(
                buffer
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<FILE_ID_BOTH_DIR_INFO>(),
            )
        };
        let len = usize::try_from(item.FileNameLength).map_err(|_| invalid())?;
        if len % 2 != 0
            || offset
                .checked_add(header)
                .and_then(|v| v.checked_add(len))
                .is_none_or(|end| end > BUFFER_SIZE)
        {
            return Err(invalid());
        }
        let mut name = Vec::with_capacity(len / 2);
        for position in (0..len).step_by(2) {
            // SAFETY: the entire name range was checked above.
            name.push(unsafe {
                std::ptr::read_unaligned(
                    buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset + header + position)
                        .cast::<u16>(),
                )
            });
        }
        if name_matches(&name, wanted) {
            if found.is_some() {
                return Err(invalid());
            }
            *found = Some(Entry {
                attributes: item.FileAttributes,
                inode: item.FileId as u64,
            });
        }
        if item.NextEntryOffset == 0 {
            break;
        }
        let next = usize::try_from(item.NextEntryOffset).map_err(|_| invalid())?;
        if next < header + len {
            return Err(invalid());
        }
        offset = offset.checked_add(next).ok_or_else(invalid)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
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
        // The same bit has another meaning outside directory enumeration.
        assert!(check_handle_attributes(FILE_ATTRIBUTE_RECALL_ON_OPEN.0).is_ok());
        assert!(check_handle_attributes(FILE_ATTRIBUTE_OFFLINE.0).is_err());
    }
    #[test]
    fn scope_name_matching_does_not_guess_non_ascii_case_rules() {
        assert!(name_matches(&[65, 0xd800], &[97, 0xd800]));
        assert!(!name_matches(&[0x00c9], &[0x00e9]));
        assert!(!name_matches(&[65], &[65, 66]));
    }
}
