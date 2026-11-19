//! Verification of volume-root mount points without following their targets.
use super::super::{identity, raw};
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{MetadataObservation, ObservedKind},
};
use std::fs::File;
use windows::Win32::{
    Storage::FileSystem::BY_HANDLE_FILE_INFORMATION,
    System::{IO::DeviceIoControl, Ioctl::FSCTL_GET_REPARSE_POINT},
};

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const FIXED_BUFFER_SIZE: usize = 16;
const MAX_REPARSE_BUFFER_SIZE: usize = 16 * 1024;

/// Returns identity-only metadata when the entry contains an exact volume-GUID root.
pub(super) fn volume_root_metadata(
    file: &File,
    value: &BY_HANDLE_FILE_INFORMATION,
    parent_volume: u64,
    enumerated_inode: u64,
) -> Result<Option<MetadataObservation>, PortError> {
    if !is_volume_root_mount_point(file)
        || enumerated_inode == 0
        || u64::from(value.dwVolumeSerialNumber) != parent_volume
        || identity(value) != enumerated_inode
    {
        return Ok(None);
    }

    Ok(Some(MetadataObservation {
        kind: ObservedKind::MountPoint,
        device: parent_volume,
        inode: enumerated_inode,
        size: Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        }),
        modified_at: None,
        native_modified_at: None,
        allocated: None,
        link_count: None,
        space_identity: None,
    }))
}

fn is_volume_root_mount_point(file: &File) -> bool {
    let mut output = [0u8; MAX_REPARSE_BUFFER_SIZE];
    let mut returned = 0u32;
    // SAFETY: file owns a live synchronous handle and output is writable for its length.
    let result = unsafe {
        DeviceIoControl(
            raw(file),
            FSCTL_GET_REPARSE_POINT,
            None,
            0,
            Some(output.as_mut_ptr().cast()),
            output.len() as u32,
            Some(&mut returned),
            None,
        )
    };
    result.is_ok()
        && usize::try_from(returned)
            .ok()
            .and_then(|length| output.get(..length))
            .is_some_and(is_volume_root_mount_point_bytes)
}

fn is_volume_root_mount_point_bytes(raw: &[u8]) -> bool {
    parse_substitute_name(raw)
        .as_deref()
        .is_some_and(is_exact_volume_guid_root)
}

fn parse_substitute_name(raw: &[u8]) -> Option<String> {
    if raw.len() < FIXED_BUFFER_SIZE
        || u32::from_le_bytes(raw.get(..4)?.try_into().ok()?) != IO_REPARSE_TAG_MOUNT_POINT
    {
        return None;
    }

    let data_length = usize::from(u16::from_le_bytes(raw.get(4..6)?.try_into().ok()?));
    if 8usize.checked_add(data_length)? != raw.len() || data_length < 8 {
        return None;
    }
    let path = raw.get(FIXED_BUFFER_SIZE..)?;
    if !path.len().is_multiple_of(2) {
        return None;
    }

    let substitute_offset = u16::from_le_bytes(raw.get(8..10)?.try_into().ok()?);
    let substitute_length = u16::from_le_bytes(raw.get(10..12)?.try_into().ok()?);
    let print_offset = u16::from_le_bytes(raw.get(12..14)?.try_into().ok()?);
    let print_length = u16::from_le_bytes(raw.get(14..16)?.try_into().ok()?);
    let substitute = name_range(path, substitute_offset, substitute_length, true)?;
    let print = name_range(path, print_offset, print_length, false)?;
    if !print.is_empty() && String::from_utf16(&utf16_units(print)?).is_err() {
        return None;
    }
    String::from_utf16(&utf16_units(substitute)?).ok()
}

fn name_range(path: &[u8], offset: u16, length: u16, required: bool) -> Option<&[u8]> {
    let offset = usize::from(offset);
    let length = usize::from(length);
    if offset % 2 != 0 || length % 2 != 0 || (required && length == 0) {
        return None;
    }
    path.get(offset..offset.checked_add(length)?)
}

fn utf16_units(bytes: &[u8]) -> Option<Vec<u16>> {
    let (units, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    Some(units.iter().map(|unit| u16::from_le_bytes(*unit)).collect())
}

fn is_exact_volume_guid_root(name: &str) -> bool {
    const PREFIX: &[u8] = br"\??\Volume{";
    let bytes = name.as_bytes();
    let guid_start = PREFIX.len();
    let guid_end = guid_start + 36;
    if bytes.len() != guid_end + 2
        || !bytes.starts_with(PREFIX)
        || bytes.get(guid_end..).is_none_or(|suffix| suffix != b"}\\")
    {
        return false;
    }
    bytes[guid_start..guid_end]
        .iter()
        .enumerate()
        .all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests {
    use super::{
        FIXED_BUFFER_SIZE, IO_REPARSE_TAG_MOUNT_POINT, is_volume_root_mount_point_bytes,
    };

    fn mount_point(substitute: &[u16]) -> Vec<u8> {
        let path = substitute
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect::<Vec<_>>();
        let data_length = 8 + path.len();
        let mut raw = vec![0u8; FIXED_BUFFER_SIZE + path.len()];
        raw[0..4].copy_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        raw[4..6].copy_from_slice(&(data_length as u16).to_le_bytes());
        raw[8..10].copy_from_slice(&0u16.to_le_bytes());
        raw[10..12].copy_from_slice(&(path.len() as u16).to_le_bytes());
        raw[12..14].copy_from_slice(&(path.len() as u16).to_le_bytes());
        raw[14..16].copy_from_slice(&0u16.to_le_bytes());
        raw[FIXED_BUFFER_SIZE..].copy_from_slice(&path);
        raw
    }

    fn target(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }

    fn volume_root() -> Vec<u8> {
        mount_point(&target(r"\??\Volume{01234567-89ab-cdef-0123-456789abcdef}\"))
    }

    #[test]
    fn accepts_only_an_exact_native_volume_guid_root() {
        assert!(is_volume_root_mount_point_bytes(&volume_root()));
        assert!(!is_volume_root_mount_point_bytes(&mount_point(&target(
            r"\??\Volume{01234567-89ab-cdef-0123-456789abcdef}\folder"
        ))));
    }

    #[test]
    fn rejects_truncation_and_out_of_range_lengths() {
        let mut raw = volume_root();
        assert!(!is_volume_root_mount_point_bytes(
            &raw[..FIXED_BUFFER_SIZE - 1]
        ));
        raw.pop();
        assert!(!is_volume_root_mount_point_bytes(&raw));

        let mut raw = volume_root();
        raw[10..12].copy_from_slice(&(u16::MAX - 1).to_le_bytes());
        assert!(!is_volume_root_mount_point_bytes(&raw));

        let mut raw = volume_root();
        raw[8..10].copy_from_slice(&(u16::MAX - 1).to_le_bytes());
        assert!(!is_volume_root_mount_point_bytes(&raw));
    }

    #[test]
    fn rejects_odd_offsets_and_lengths() {
        let mut raw = volume_root();
        raw[8..10].copy_from_slice(&1u16.to_le_bytes());
        assert!(!is_volume_root_mount_point_bytes(&raw));

        let mut raw = volume_root();
        let length = u16::from_le_bytes([raw[10], raw[11]]);
        raw[10..12].copy_from_slice(&(length - 1).to_le_bytes());
        assert!(!is_volume_root_mount_point_bytes(&raw));
    }

    #[test]
    fn rejects_invalid_utf16_and_other_tags() {
        assert!(!is_volume_root_mount_point_bytes(&mount_point(&[0xD800])));

        let mut raw = volume_root();
        raw[0..4].copy_from_slice(&0xA000_000Cu32.to_le_bytes());
        assert!(!is_volume_root_mount_point_bytes(&raw));
    }

    #[test]
    fn rejects_non_guid_paths_and_malformed_guids() {
        assert!(!is_volume_root_mount_point_bytes(&mount_point(&target(r"\??\C:\"))));
        assert!(!is_volume_root_mount_point_bytes(&mount_point(&target(
            r"\??\Volume{01234567-89ab-cdef-0123-456789abcdeg}\"
        ))));
    }
}
