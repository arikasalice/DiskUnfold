//! Kind of a directory enumeration record, including Cloud Files placeholders.
use super::kind;
use du_core_contracts::scan_observation::ObservedKind;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

/// `IO_REPARSE_TAG_CLOUD` from `winnt.h`, the first of the sixteen Cloud Files tags.
const CLOUD_TAG: u32 = 0x9000_001A;
/// `IO_REPARSE_TAG_CLOUD_MASK` from `winnt.h`: the bits that tell the sixteen tags apart.
const CLOUD_TAG_VARIANT_MASK: u32 = 0x0000_F000;

/// Whether a reparse tag is one of `IO_REPARSE_TAG_CLOUD` to `IO_REPARSE_TAG_CLOUD_F`.
fn is_cloud_tag(tag: u32) -> bool {
    tag & !CLOUD_TAG_VARIANT_MASK == CLOUD_TAG
}

/// The kind reported for one `FileIdBothDirectoryInfo` record.
///
/// A Cloud Files sync root and its placeholders are ordinary directories and
/// files that carry a reparse point owned by the cloud filter. Windows hides
/// that reparse point on a handle opened by a process in the disguised
/// placeholder mode, but the record of a sync root in its parent directory
/// keeps the attribute and the tag in every mode. Such a record is classified
/// by its directory attribute so that it agrees with the opened handle.
///
/// `ea_size` is the record's `EaSize` field, which holds the reparse tag only
/// when the record has `FILE_ATTRIBUTE_REPARSE_POINT`; otherwise it is an
/// extended-attribute size and is not read. Every other tag, and a missing
/// tag, stays a `Link`. The recall guard and the no-recall open do not depend
/// on this kind, and a handle that still reports a reparse point remains a
/// `Link`, so the collector records a disagreement instead of entering it.
pub(super) fn enumerated_kind(attributes: u32, ea_size: u32) -> ObservedKind {
    let reparse = FILE_ATTRIBUTE_REPARSE_POINT.0;
    if attributes & reparse != 0 && is_cloud_tag(ea_size) {
        kind(attributes & !reparse)
    } else {
        kind(attributes)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{entry_open_options, open_verified};
    use super::*;
    use du_core_contracts::ports::PortErrorKind;
    use du_core_contracts::scan_observation::DirectoryObservations;
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    };

    const DIRECTORY: u32 = 0x10;
    const ARCHIVE: u32 = 0x20;
    const REPARSE: u32 = 0x400;
    const MOUNT_POINT: u32 = 0xA000_0003;
    const SYMLINK: u32 = 0xA000_000C;
    /// `IO_REPARSE_TAG_CLOUD_7`, the tag observed on a sync root directory.
    const CLOUD_7: u32 = 0x9000_701A;

    fn utf16(name: &str) -> Vec<u8> {
        name.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn all_sixteen_cloud_tags_are_recognized() {
        for variant in 0..16u32 {
            assert!(is_cloud_tag(0x9000_001A | (variant << 12)), "{variant:X}");
        }
    }

    #[test]
    fn other_tags_and_near_misses_are_not_cloud_tags() {
        for tag in [
            MOUNT_POINT,
            SYMLINK,
            0x8000_001B, // IO_REPARSE_TAG_APPEXECLINK
            0x8000_0013, // IO_REPARSE_TAG_DEDUP
            0x8000_0018, // IO_REPARSE_TAG_WCI
            0x9000_1018, // IO_REPARSE_TAG_WCI_1
            0x9000_001C, // IO_REPARSE_TAG_PROJFS
            0x9000_001B,
            0x9000_0019,
            0x8000_001A,
            0xA000_001A,
            0xB000_001A,
            0x1000_001A,
            0x9001_001A,
            0x9100_001A,
            0x9000_011A,
            0x0000_001A,
            0x0000_F000,
            0,
            u32::MAX,
        ] {
            assert!(!is_cloud_tag(tag), "{tag:#010X}");
        }
    }

    #[test]
    fn cloud_records_follow_the_directory_attribute() {
        for tag in [CLOUD_TAG, CLOUD_7, 0x9000_F01A] {
            let directory = enumerated_kind(REPARSE | DIRECTORY, tag);
            assert_eq!(directory, ObservedKind::Directory, "{tag:#010X}");
            for attributes in [REPARSE, REPARSE | ARCHIVE] {
                let file = enumerated_kind(attributes, tag);
                assert_eq!(file, ObservedKind::File, "{tag:#010X}");
            }
        }
    }

    #[test]
    fn other_reparse_records_stay_links() {
        for tag in [
            MOUNT_POINT,
            SYMLINK,
            0x8000_001B,
            0x8000_0018,
            0x9000_001B,
            0,
        ] {
            for attributes in [REPARSE, REPARSE | ARCHIVE, REPARSE | DIRECTORY] {
                let observed = enumerated_kind(attributes, tag);
                assert_eq!(observed, ObservedKind::Link, "{tag:#010X}");
            }
        }
    }

    #[test]
    fn extended_attribute_size_is_not_read_as_a_tag() {
        // Without the reparse attribute EaSize is a size, whatever its value.
        for ea_size in [0, 64, CLOUD_TAG, CLOUD_7, MOUNT_POINT, SYMLINK] {
            assert_eq!(enumerated_kind(ARCHIVE, ea_size), ObservedKind::File);
            assert_eq!(enumerated_kind(DIRECTORY, ea_size), ObservedKind::Directory);
        }
    }

    #[test]
    fn handle_attributes_with_a_reparse_point_remain_links() {
        // The handle kind has no tag input: an exposed placeholder stays a Link
        // and so disagrees with its enumerated kind.
        assert_eq!(kind(REPARSE | DIRECTORY), ObservedKind::Link);
        assert_eq!(kind(REPARSE | ARCHIVE), ObservedKind::Link);
    }

    #[test]
    fn a_cloud_tag_never_bypasses_the_recall_guard() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = tempfile::tempdir()?;
        let directory = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
        let recall = [
            FILE_ATTRIBUTE_RECALL_ON_OPEN.0,
            FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0,
            FILE_ATTRIBUTE_OFFLINE.0,
        ];
        for flag in recall {
            for (base, expected) in [
                (REPARSE | ARCHIVE, ObservedKind::File),
                (REPARSE | DIRECTORY, ObservedKind::Directory),
            ] {
                // The name does not exist: an attempted open would report
                // NotFound instead of the guard's Unsupported.
                let entry = directory.observe(utf16("absent"), 7, base | flag, CLOUD_TAG);
                assert_eq!(entry.enumerated_kind, expected);
                let error = entry.metadata.err().ok_or("guard did not refuse")?;
                assert_eq!(error.kind, PortErrorKind::Unsupported);
                assert_eq!(error.reason_key, "os.unsupported");
            }
        }
        // Without a recall flag the same record is opened.
        let opened = directory.observe(utf16("absent"), 7, REPARSE | ARCHIVE, CLOUD_TAG);
        assert_eq!(
            opened.metadata.err().map(|error| error.kind),
            Some(PortErrorKind::NotFound)
        );
        Ok(())
    }

    #[test]
    fn a_sync_root_record_agrees_with_its_disguised_handle()
    -> Result<(), Box<dyn std::error::Error>> {
        // An ordinary directory stands in for the disguised handle of a sync
        // root: the listing reports the cloud tag and the handle reports none.
        let fixture = tempfile::tempdir()?;
        std::fs::create_dir(fixture.path().join("root"))?;
        let mut directory = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
        let listed = directory.next_batch().map_err(|e| format!("{e:?}"))?;
        let inode = listed.first().ok_or("missing entry")?.enumerated_inode;
        let entry = directory.observe(utf16("root"), inode, REPARSE | DIRECTORY, CLOUD_7);
        let handle = entry.metadata.map_err(|e| format!("{e:?}"))?;
        assert_eq!(entry.enumerated_kind, ObservedKind::Directory);
        assert_eq!(handle.kind, ObservedKind::Directory);
        assert_eq!(handle.inode, inode);
        // The same record with a junction's tag keeps today's disagreement.
        let link = directory.observe(utf16("root"), inode, REPARSE | DIRECTORY, MOUNT_POINT);
        assert_eq!(link.enumerated_kind, ObservedKind::Link);
        Ok(())
    }

    #[test]
    fn entries_are_still_opened_without_recall_or_reparse_traversal() {
        // FILE_OPEN_REPARSE_POINT | FILE_OPEN_NO_RECALL | FILE_SYNCHRONOUS_IO_NONALERT
        assert_eq!(
            entry_open_options().0,
            0x0020_0000 | 0x0040_0000 | 0x0000_0020
        );
    }
}
