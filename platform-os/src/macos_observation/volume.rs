//! Volume identity and space read through an already open descriptor.
//!
//! The kernel packs volume attributes in its own order, which is not bit order:
//! macOS 15 and later place `ATTR_VOL_SPACEUSED` before `ATTR_VOL_UUID`. Each
//! request here therefore asks for one volume attribute, together with the file
//! system ID from the common group, which always precedes the volume group. A
//! volume request on any directory is answered from the root of its volume.
use super::{bulk::Cursor, errors};
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{VolumeSpace, VolumeSpaceReader},
};
use libc::{
    ATTR_BIT_MAP_COUNT, ATTR_CMN_FSID, ATTR_CMN_RETURNED_ATTRS, ATTR_DIR_MOUNTSTATUS,
    ATTR_VOL_INFO, ATTR_VOL_SPACEUSED, ATTR_VOL_UUID, DIR_MNTSTATUS_MNTPOINT,
};
use std::{
    fmt::Write,
    os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd},
};

/// File system ID bytes in the layout of `fsid_t`.
type Fsid = [u8; 8];

/// A reply to one volume attribute request.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct VolumeReply<const N: usize> {
    /// The file system ID of the volume that answered.
    pub(super) fsid: Fsid,
    /// `None` when the file system did not return the attribute.
    pub(super) value: Option<[u8; N]>,
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

fn request(common: u32, volume: u32, directory: u32) -> libc::attrlist {
    libc::attrlist {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: ATTR_CMN_RETURNED_ATTRS | common,
        volattr: volume,
        dirattr: directory,
        fileattr: 0,
        forkattr: 0,
    }
}

/// Requests the file system ID and the single volume attribute `attribute`,
/// whose value is `N` bytes. `Ok(None)` means the file system declines volume
/// attribute requests.
fn volume_attribute<const N: usize>(
    fd: BorrowedFd<'_>,
    attribute: u32,
) -> Result<Option<VolumeReply<N>>, PortError> {
    // Length, returned groups, file system ID and up to 16 value bytes.
    let mut reply = [0u8; 48];
    match attributes(
        fd,
        request(ATTR_CMN_FSID, ATTR_VOL_INFO | attribute, 0),
        &mut reply,
    ) {
        Ok(()) => parse_volume(&reply, attribute).map(Some),
        Err(error) if declines_volume_requests(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Codes with which a file system declines volume attribute requests.
fn declines_volume_requests(error: &PortError) -> bool {
    matches!(
        error.os_code,
        Some(libc::EINVAL | libc::ENOTSUP | libc::EOPNOTSUPP | libc::ENOSYS)
    )
}

/// Parses a reply to one volume attribute request. Any other returned volume
/// attribute would change the layout, so such a reply is rejected.
pub(super) fn parse_volume<const N: usize>(
    reply: &[u8],
    attribute: u32,
) -> Result<VolumeReply<N>, PortError> {
    let length = usize::try_from(Cursor::new(reply, 0).u32()?).map_err(|_| errors::invalid())?;
    let reply = reply.get(..length).ok_or_else(errors::invalid)?;
    let mut cursor = Cursor::new(reply, 4);
    let [common, volume, directory, file, fork] = cursor.returned()?;
    if common & !(ATTR_CMN_RETURNED_ATTRS | ATTR_CMN_FSID) != 0
        || volume & !(ATTR_VOL_INFO | attribute) != 0
        || directory | file | fork != 0
    {
        return Err(errors::invalid());
    }
    let fsid = cursor
        .optional(common & ATTR_CMN_FSID, Cursor::take::<8>)?
        .ok_or_else(errors::unavailable)?;
    let value = cursor.optional(volume & attribute, Cursor::take::<N>)?;
    Ok(VolumeReply { fsid, value })
}

/// `uuid:` and the volume UUID when the volume reports one, otherwise `dev:`
/// and the device number held in the file system ID.
pub(super) fn format(fsid: Fsid, uuid: Option<[u8; 16]>) -> Vec<u8> {
    let mut text = String::new();
    match uuid.filter(|uuid| uuid.iter().any(|&byte| byte != 0)) {
        Some(uuid) => {
            text.push_str("uuid:");
            for (index, byte) in uuid.iter().enumerate() {
                if matches!(index, 4 | 6 | 8 | 10) {
                    text.push('-');
                }
                let _ = write!(text, "{byte:02X}");
            }
        }
        None => {
            let [a, b, c, d, ..] = fsid;
            let _ = write!(text, "dev:{:08x}", u32::from_ne_bytes([a, b, c, d]));
        }
    }
    text.into_bytes()
}

/// The file system ID of `stats`, in the layout attribute replies use.
fn fsid(stats: &libc::statfs) -> Fsid {
    // SAFETY: `fsid_t` is a `repr(C)` struct holding only `[i32; 2]`, so it has
    // the size of `Fsid` and each of its bit patterns is a valid byte array.
    unsafe { std::mem::transmute::<libc::fsid_t, Fsid>(stats.f_fsid) }
}

/// Identity of the volume holding `fd`: `uuid:` and the volume UUID, or `dev:`
/// and the device number of the file system ID when the volume reports no UUID.
///
/// The value depends only on the volume, never on which of its directories
/// `fd` refers to.
pub(super) fn identity(fd: BorrowedFd<'_>) -> Result<Vec<u8>, PortError> {
    match volume_attribute::<16>(fd, ATTR_VOL_UUID)? {
        Some(reply) => Ok(format(reply.fsid, reply.value)),
        None => {
            let stats = rustix::fs::fstatfs(fd).map_err(errors::os)?;
            Ok(format(fsid(&stats), None))
        }
    }
}

/// Whether `fd` is the root directory of a mounted file system.
pub(super) fn is_mount_root(fd: BorrowedFd<'_>) -> Result<bool, PortError> {
    let mut reply = [0u8; 28];
    attributes(fd, request(0, 0, ATTR_DIR_MOUNTSTATUS), &mut reply)?;
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
    let reported = volume_attribute::<8>(fd, ATTR_VOL_SPACEUSED)?
        .and_then(|reply| reply.value)
        .and_then(|bytes| u64::try_from(i64::from_ne_bytes(bytes)).ok());
    let used = match reported {
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
