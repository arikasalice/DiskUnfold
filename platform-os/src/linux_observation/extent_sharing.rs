//! Whether the file system of a verified directory can share blocks between files.
use rustix::{
    fd::AsFd,
    fs::{StatVfsMountFlags, fstatfs, fstatvfs},
};

/// `EXT4_SUPER_MAGIC`, which ext2, ext3 and ext4 share.
const EXT_MAGIC: i128 = 0xEF53;

/// True only for an ext2, ext3 or ext4 volume mounted read-write (F03 FR11.HD1).
///
/// ext4 has no reflink or block clone, so a file's data blocks belong to that file
/// alone; hard links are file identity, not sharing. The read-only
/// `shared_blocks` feature lets deduplicated images share data blocks, and the
/// kernel refuses to mount such a volume read-write, so a read-only volume stays
/// unknown. Extended-attribute blocks that ext4 shares between inodes with the same
/// attributes are counted in each file's `st_blocks`, as `du` counts them; that is
/// a stated limit of the allocated size, not data sharing. A failed query, another
/// file system (btrfs, XFS, overlayfs, ZFS and others) leaves sharing unknown.
pub(super) fn creates_no_shared_extents(fd: impl AsFd) -> bool {
    let Ok(volume) = fstatfs(&fd) else {
        return false;
    };
    let Ok(mount) = fstatvfs(&fd) else {
        return false;
    };
    no_shared_extents(
        i128::from(volume.f_type),
        mount.f_flag.contains(StatVfsMountFlags::RDONLY),
    )
}

fn no_shared_extents(magic: i128, read_only: bool) -> bool {
    magic == EXT_MAGIC && !read_only
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_read_write_ext_volume_shares_nothing() {
        assert!(no_shared_extents(0xEF53, false));
        assert!(!no_shared_extents(0xEF53, true));
        // btrfs, XFS, tmpfs and overlayfs.
        for magic in [0x9123_683E, 0x5846_5342, 0x0102_1994, 0x794C_7630] {
            assert!(!no_shared_extents(magic, false), "{magic:#x}");
        }
    }
}
