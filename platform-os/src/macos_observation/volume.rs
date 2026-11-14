//! Volume identity and space read through an already open descriptor.
use super::{bulk::Cursor, errors};
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{VolumeSpace, VolumeSpaceReader},
};
use libc::{
    ATTR_BIT_MAP_COUNT, ATTR_CMN_RETURNED_ATTRS, ATTR_DIR_MOUNTSTATUS, ATTR_VOL_INFO,
    ATTR_VOL_SPACEUSED, ATTR_VOL_UUID, DIR_MNTSTATUS_MNTPOINT,
};
use std::{
    fmt::Write,
    os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd},
};

/// Volume attributes; `None` means the file system did not return the value.
struct Facts {
    uuid: Option<[u8; 16]>,
    used: Option<u64>,
}

fn attributes(
    fd: BorrowedFd<'_>,
    mut list: libc::attrlist,
    reply: &mut [u8],
) -> Result<(), PortError> {
    // SAFETY: `list` and `reply` are live and writable for the call, the length
    // is the exact size of `reply`, and `fd` stays open for its duration.
    let status = unsafe {
        libc::fgetattrlist(
            fd.as_raw_fd(),
            (&raw mut list).cast(),
            reply.as_mut_ptr().cast(),
            reply.len(),
            0,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(errors::last())
    }
}

fn request(volume: u32, directory: u32) -> libc::attrlist {
    libc::attrlist {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: ATTR_CMN_RETURNED_ATTRS,
        volattr: volume,
        dirattr: directory,
        fileattr: 0,
        forkattr: 0,
    }
}

fn facts(fd: BorrowedFd<'_>) -> Result<Facts, PortError> {
    // Length, returned groups, UUID and used bytes, in attribute bit order.
    let mut reply = [0u8; 48];
    let list = request(ATTR_VOL_INFO | ATTR_VOL_UUID | ATTR_VOL_SPACEUSED, 0);
    match attributes(fd, list, &mut reply) {
        Ok(()) => {}
        // Some file systems reject volume attribute requests with EINVAL.
        Err(error)
            if error.kind == PortErrorKind::Unsupported || error.os_code == Some(libc::EINVAL) =>
        {
            return Ok(Facts {
                uuid: None,
                used: None,
            });
        }
        Err(error) => return Err(error),
    }
    let mut cursor = Cursor::new(&reply, 4);
    let [_, volume, ..] = cursor.returned()?;
    let uuid = cursor
        .optional(volume & ATTR_VOL_UUID, Cursor::take::<16>)?
        .filter(|uuid| uuid.iter().any(|&byte| byte != 0));
    let used = cursor
        .optional(volume & ATTR_VOL_SPACEUSED, Cursor::i64)?
        .and_then(|bytes| u64::try_from(bytes).ok());
    Ok(Facts { uuid, used })
}

/// Identity of the actual volume holding `fd`: `uuid:` and the volume UUID, or
/// `dev:` and the device number when the file system reports no UUID.
pub(super) fn identity(fd: BorrowedFd<'_>) -> Result<Vec<u8>, PortError> {
    let mut text = String::new();
    if let Some(uuid) = facts(fd)?.uuid {
        text.push_str("uuid:");
        for (index, byte) in uuid.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                text.push('-');
            }
            let _ = write!(text, "{byte:02X}");
        }
    } else {
        let stat = rustix::fs::fstat(fd).map_err(errors::os)?;
        let _ = write!(text, "dev:{:08x}", stat.st_dev.cast_unsigned());
    }
    Ok(text.into_bytes())
}

/// Whether `fd` is the root directory of a mounted file system.
pub(super) fn is_mount_root(fd: BorrowedFd<'_>) -> Result<bool, PortError> {
    let mut reply = [0u8; 28];
    attributes(fd, request(0, ATTR_DIR_MOUNTSTATUS), &mut reply)?;
    let mut cursor = Cursor::new(&reply, 4);
    let [_, _, directory, ..] = cursor.returned()?;
    Ok(cursor
        .optional(directory & ATTR_DIR_MOUNTSTATUS, Cursor::u32)?
        .is_some_and(|status| status & DIR_MNTSTATUS_MNTPOINT != 0))
}

/// Re-observes one volume through a duplicate of a verified directory descriptor.
struct Reader {
    fd: OwnedFd,
}

pub(super) fn reader(fd: BorrowedFd<'_>) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
    let fd = rustix::io::fcntl_dupfd_cloexec(fd, 0).map_err(errors::os)?;
    Ok(Box::new(Reader { fd }))
}

impl VolumeSpaceReader for Reader {
    /// Total and free bytes are those of the volume's container on APFS; used
    /// bytes are the volume's own usage when the file system reports it.
    fn read(&self, expected_volume: &[u8]) -> Result<VolumeSpace, PortError> {
        checked(
            expected_volume,
            || identity(self.fd.as_fd()),
            || space(self.fd.as_fd()),
        )
        .map_err(|error| PortError {
            reason_key: "space.volume_report_unavailable".into(),
            ..error
        })
    }
}

fn space(fd: BorrowedFd<'_>) -> Result<VolumeSpace, PortError> {
    let stats = rustix::fs::fstatfs(fd).map_err(errors::os)?;
    let block = u64::from(stats.f_bsize);
    let total = stats
        .f_blocks
        .checked_mul(block)
        .ok_or_else(errors::invalid)?;
    let free = stats
        .f_bfree
        .checked_mul(block)
        .ok_or_else(errors::invalid)?;
    let used = match facts(fd)?.used {
        Some(used) => used,
        None => total.checked_sub(free).ok_or_else(errors::invalid)?,
    };
    Ok(VolumeSpace {
        total_bytes: total,
        used_bytes: used,
        free_bytes: free,
    })
}

/// Reads only while the descriptor still resolves to the expected volume.
pub(super) fn checked<T>(
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

fn changed() -> PortError {
    PortError {
        kind: PortErrorKind::IdentityChanged,
        reason_key: "scan.disconnected".into(),
        os_code: None,
    }
}
