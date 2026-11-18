//! One bounded `getattrlistbulk` request and the parsing of its packed records.
use super::{dataless, errors};
use du_core_contracts::{
    Timestamp,
    ports::{PortError, PortErrorKind},
    scan_observation::ObservedKind,
};
use libc::{
    ATTR_BIT_MAP_COUNT, ATTR_CMN_DEVID, ATTR_CMN_FILEID, ATTR_CMN_FLAGS, ATTR_CMN_MODTIME,
    ATTR_CMN_NAME, ATTR_CMN_OBJTYPE, ATTR_CMN_RETURNED_ATTRS, ATTR_DIR_DATALENGTH,
    ATTR_DIR_MOUNTSTATUS, ATTR_FILE_ALLOCSIZE, ATTR_FILE_DATALENGTH, ATTR_FILE_LINKCOUNT,
    DIR_MNTSTATUS_MNTPOINT,
};
use std::os::fd::{AsRawFd, BorrowedFd};

/// Bytes offered to one bulk call; it returns as many whole records as fit.
pub(super) const BUFFER_BYTES: usize = 64 * 1024;

// <sys/attr.h> and <sys/stat.h> values that libc 0.2.191 does not export.
const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const DIR_MNTSTATUS_TRIGGER: u32 = 0x2;
const SF_FIRMLINK: u32 = 0x0080_0000;
// <sys/vnode.h> `enum vtype`.
const VNON: u32 = 0;
const VREG: u32 = 1;
pub(super) const VDIR: u32 = 2;
const VLNK: u32 = 5;
const VBAD: u32 = 8;

pub(super) const COMMON: u32 = ATTR_CMN_RETURNED_ATTRS
    | ATTR_CMN_ERROR
    | ATTR_CMN_NAME
    | ATTR_CMN_DEVID
    | ATTR_CMN_OBJTYPE
    | ATTR_CMN_MODTIME
    | ATTR_CMN_FLAGS
    | ATTR_CMN_FILEID;
pub(super) const DIRECTORY: u32 = ATTR_DIR_MOUNTSTATUS | ATTR_DIR_DATALENGTH;
/// `ATTR_FILE_DATALENGTH` is the logical length of the data fork, which
/// `stat` reports as `st_size`. `ATTR_FILE_ALLOCSIZE` is the allocation of
/// every fork, which `stat` reports as `st_blocks` and `du` counts: it
/// includes a resource fork, unlike `ATTR_FILE_DATAALLOCSIZE`, and it is the
/// compressed allocation of a file with transparent APFS compression.
pub(super) const FILE: u32 = ATTR_FILE_LINKCOUNT | ATTR_FILE_ALLOCSIZE | ATTR_FILE_DATALENGTH;
/// Length word plus the five returned-attribute groups.
const HEADER: usize = 24;

/// One directory record. `None` means the file system did not return the value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Record {
    pub(super) name: Vec<u8>,
    /// Per-entry `errno` reported instead of some attributes.
    pub(super) error: Option<u32>,
    /// Device number; APFS reports one value for a whole volume group.
    pub(super) device: Option<u64>,
    pub(super) object_type: Option<u32>,
    pub(super) modified: Option<(i64, i64)>,
    pub(super) flags: Option<u32>,
    pub(super) file_id: Option<u64>,
    pub(super) mount_status: Option<u32>,
    /// Logical length of a directory, `st_size` of the directory itself.
    pub(super) directory_length: Option<i64>,
    pub(super) link_count: Option<u32>,
    /// Allocation of every fork of a non-directory.
    pub(super) allocated: Option<i64>,
    /// Logical length of the data fork of a non-directory.
    pub(super) data_length: Option<i64>,
}

impl Record {
    /// Kind for both views of the record.
    ///
    /// A directory whose mount status says that a file system is mounted on
    /// it is a mount point. Firmlinks and automount triggers that are not
    /// mounted lead elsewhere too, but the record does not establish another
    /// volume: a firmlink reports no mount status and its target may share the
    /// volume group, and a trigger mounts a file system only when it is
    /// crossed. They cannot be told apart from links and are links.
    pub(super) fn kind(&self) -> ObservedKind {
        let status = self.mount_status.unwrap_or(0);
        let firmlink = self.flags.is_some_and(|flags| flags & SF_FIRMLINK != 0);
        match self.object_type {
            Some(VREG) => ObservedKind::File,
            Some(VDIR) if firmlink => ObservedKind::Link,
            Some(VDIR) if status & DIR_MNTSTATUS_MNTPOINT != 0 => ObservedKind::MountPoint,
            Some(VDIR) if status & DIR_MNTSTATUS_TRIGGER != 0 => ObservedKind::Link,
            Some(VDIR) => ObservedKind::Directory,
            Some(VLNK) => ObservedKind::Link,
            None | Some(VNON | VBAD) => ObservedKind::Unknown,
            Some(_) => ObservedKind::Other,
        }
    }

    /// The logical length of the entry itself: the directory length for a
    /// directory and the data fork length, or link target length, otherwise.
    pub(super) fn length(&self) -> Option<i64> {
        if self.object_type == Some(VDIR) {
            self.directory_length
        } else {
            self.data_length
        }
    }
}

/// Reads the next records of `directory` into `buffer`; an empty result ends
/// the directory. `buffer` is reused across calls on one directory.
///
/// Runs with dataless materialization refused, so a dataless directory fails
/// instead of being downloaded. `EINTR` is retried.
pub(super) fn read(directory: BorrowedFd<'_>, buffer: &mut [u8]) -> Result<Vec<Record>, PortError> {
    let count = dataless::without_materialization(|| request(directory, buffer))??;
    parse(buffer, count)
}

/// Correlated reads use fallible allocations so a lost record becomes a stream error.
pub(super) fn read_correlated(
    directory: BorrowedFd<'_>,
    buffer: &mut [u8],
) -> Result<Vec<Record>, PortError> {
    let count = dataless::without_materialization(|| request(directory, buffer))??;
    parse_correlated(buffer, count)
}

pub(super) fn allocation_error() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

pub(super) fn try_copy_bytes(bytes: &[u8]) -> Result<Vec<u8>, PortError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|_| allocation_error())?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}

fn request(directory: BorrowedFd<'_>, buffer: &mut [u8]) -> Result<usize, PortError> {
    let mut list = libc::attrlist {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: COMMON,
        volattr: 0,
        dirattr: DIRECTORY,
        fileattr: FILE,
        forkattr: 0,
    };
    loop {
        // SAFETY: `list` and `buffer` are live and writable for the call, the
        // length is the exact size of `buffer`, and `directory` stays open.
        let count = unsafe {
            libc::getattrlistbulk(
                directory.as_raw_fd(),
                (&raw mut list).cast(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                0,
            )
        };
        if let Ok(count) = usize::try_from(count) {
            return Ok(count);
        }
        let error = errors::last();
        if error.os_code != Some(libc::EINTR) {
            return Err(error);
        }
    }
}

/// Parses `count` packed records; malformed or unrequested layouts are rejected.
///
/// One malformed record rejects the whole reply, including the records that
/// parsed before it: a reply that breaks the requested layout cannot be
/// trusted, and the kernel offers no way to resume after a single record.
/// The directory then ends with that error, and the collector reports it as
/// a directory that could not be read in full.
pub(super) fn parse(bytes: &[u8], count: usize) -> Result<Vec<Record>, PortError> {
    parse_with_allocation_mode(bytes, count, false)
}

pub(super) fn parse_correlated(bytes: &[u8], count: usize) -> Result<Vec<Record>, PortError> {
    parse_with_allocation_mode(bytes, count, true)
}

fn parse_with_allocation_mode(
    bytes: &[u8],
    count: usize,
    fallible: bool,
) -> Result<Vec<Record>, PortError> {
    if fallible && count > bytes.len() / HEADER {
        return Err(errors::invalid());
    }
    let mut records = if fallible {
        let mut records = Vec::new();
        records
            .try_reserve_exact(count)
            .map_err(|_| allocation_error())?;
        records
    } else {
        Vec::with_capacity(count)
    };
    let mut offset = 0usize;
    for _ in 0..count {
        let length =
            usize::try_from(Cursor::new(bytes, offset).u32()?).map_err(|_| errors::invalid())?;
        let end = offset
            .checked_add(length)
            .filter(|_| length >= HEADER)
            .ok_or_else(errors::invalid)?;
        let record = bytes.get(offset..end).ok_or_else(errors::invalid)?;
        records.push(record_from(record, fallible)?);
        offset = end;
    }
    Ok(records)
}

fn record_from(bytes: &[u8], fallible: bool) -> Result<Record, PortError> {
    let mut cursor = Cursor::new(bytes, 4);
    let [common, volume, directory, file, fork] = cursor.returned()?;
    if common & !COMMON != 0
        || directory & !DIRECTORY != 0
        || file & !FILE != 0
        || volume != 0
        || fork != 0
        || common & ATTR_CMN_NAME == 0
    {
        return Err(errors::invalid());
    }
    // ATTR_CMN_ERROR precedes the other attributes, as getattrlistbulk(2) shows.
    let error = cursor.optional(common & ATTR_CMN_ERROR, Cursor::u32)?;
    Ok(Record {
        name: cursor.name(fallible)?,
        error,
        device: cursor
            .optional(common & ATTR_CMN_DEVID, Cursor::u32)?
            .map(u64::from),
        object_type: cursor.optional(common & ATTR_CMN_OBJTYPE, Cursor::u32)?,
        modified: cursor.optional(common & ATTR_CMN_MODTIME, Cursor::timespec)?,
        flags: cursor.optional(common & ATTR_CMN_FLAGS, Cursor::u32)?,
        file_id: cursor.optional(common & ATTR_CMN_FILEID, Cursor::u64)?,
        mount_status: cursor.optional(directory & ATTR_DIR_MOUNTSTATUS, Cursor::u32)?,
        directory_length: cursor.optional(directory & ATTR_DIR_DATALENGTH, Cursor::i64)?,
        // File attributes follow bit order: link count, allocation, length.
        link_count: cursor.optional(file & ATTR_FILE_LINKCOUNT, Cursor::u32)?,
        allocated: cursor.optional(file & ATTR_FILE_ALLOCSIZE, Cursor::i64)?,
        data_length: cursor.optional(file & ATTR_FILE_DATALENGTH, Cursor::i64)?,
    })
}

/// Bounds-checked reader over packed attribute values in native byte order.
pub(super) struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(bytes: &'a [u8], at: usize) -> Self {
        Self { bytes, at }
    }

    pub(super) fn take<const N: usize>(&mut self) -> Result<[u8; N], PortError> {
        let end = self.at.checked_add(N).ok_or_else(errors::invalid)?;
        let value = self
            .bytes
            .get(self.at..end)
            .and_then(|bytes| <[u8; N]>::try_from(bytes).ok())
            .ok_or_else(errors::invalid)?;
        self.at = end;
        Ok(value)
    }

    pub(super) fn u32(&mut self) -> Result<u32, PortError> {
        self.take().map(u32::from_ne_bytes)
    }

    fn u64(&mut self) -> Result<u64, PortError> {
        self.take().map(u64::from_ne_bytes)
    }

    pub(super) fn i64(&mut self) -> Result<i64, PortError> {
        self.take().map(i64::from_ne_bytes)
    }

    fn timespec(&mut self) -> Result<(i64, i64), PortError> {
        Ok((self.i64()?, self.i64()?))
    }

    /// The common, volume, directory, file and fork groups that were returned.
    pub(super) fn returned(&mut self) -> Result<[u32; 5], PortError> {
        Ok([
            self.u32()?,
            self.u32()?,
            self.u32()?,
            self.u32()?,
            self.u32()?,
        ])
    }

    /// Reads a value only when its returned bit is set.
    pub(super) fn optional<T>(
        &mut self,
        returned: u32,
        read: impl FnOnce(&mut Self) -> Result<T, PortError>,
    ) -> Result<Option<T>, PortError> {
        if returned == 0 {
            return Ok(None);
        }
        read(self).map(Some)
    }

    /// Reads an `attrreference_t` that points at a NUL-terminated entry name.
    fn name(&mut self, fallible: bool) -> Result<Vec<u8>, PortError> {
        let base = self.at;
        let offset = i32::from_ne_bytes(self.take()?);
        let length = usize::try_from(self.u32()?).map_err(|_| errors::invalid())?;
        let start = usize::try_from(offset)
            .ok()
            .and_then(|offset| base.checked_add(offset))
            .ok_or_else(errors::invalid)?;
        let end = start.checked_add(length).ok_or_else(errors::invalid)?;
        let (terminator, name) = self
            .bytes
            .get(start..end)
            .and_then(<[u8]>::split_last)
            .ok_or_else(errors::invalid)?;
        if *terminator != 0 || name.is_empty() || name.contains(&0) || name.contains(&b'/') {
            return Err(errors::invalid());
        }
        if fallible {
            try_copy_bytes(name)
        } else {
            Ok(name.to_vec())
        }
    }
}

/// Converts a native `timespec` to a UTC timestamp, flooring to milliseconds.
pub(super) fn timestamp(seconds: i64, nanoseconds: i64) -> Option<Timestamp> {
    let millis = seconds
        .checked_mul(1000)?
        .checked_add(nanoseconds.div_euclid(1_000_000))?;
    Timestamp::from_unix_millis(millis)
}
