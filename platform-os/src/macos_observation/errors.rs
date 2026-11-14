//! Neutral classification of macOS `errno` values; OS error text is never kept.
use du_core_contracts::ports::{PortError, PortErrorKind};

/// First matching row wins; any other value is a generic query failure.
const TABLE: [(i32, PortErrorKind, &str); 14] = [
    (libc::ENOENT, PortErrorKind::NotFound, "os.query_failed"),
    (
        libc::EACCES,
        PortErrorKind::PermissionDenied,
        "os.query_failed",
    ),
    // Privacy protection, including missing Full Disk Access, reports EPERM.
    (
        libc::EPERM,
        PortErrorKind::PermissionDenied,
        "os.query_failed",
    ),
    // Explicit reports that the device or its connection is gone.
    (libc::ENXIO, PortErrorKind::Io, "scan.disconnected"),
    (libc::ENODEV, PortErrorKind::Io, "scan.disconnected"),
    (libc::ENOTCONN, PortErrorKind::Io, "scan.disconnected"),
    // A link or another non-directory where a directory was required.
    (
        libc::ENOTDIR,
        PortErrorKind::Unsupported,
        "path.unresolvable",
    ),
    (libc::ELOOP, PortErrorKind::Unsupported, "path.unresolvable"),
    // The thread I/O policy refused to materialize a dataless object.
    (libc::EDEADLK, PortErrorKind::Unsupported, "os.unsupported"),
    (libc::ENOTSUP, PortErrorKind::Unsupported, "os.unsupported"),
    (
        libc::EOPNOTSUPP,
        PortErrorKind::Unsupported,
        "os.unsupported",
    ),
    (libc::ENOSYS, PortErrorKind::Unsupported, "os.unsupported"),
    (libc::EAGAIN, PortErrorKind::Busy, "os.query_failed"),
    (libc::EBUSY, PortErrorKind::Busy, "os.query_failed"),
];

pub(super) fn errno(code: i32) -> PortError {
    let (kind, reason) = TABLE
        .iter()
        .find(|row| row.0 == code)
        .map_or((PortErrorKind::Io, "os.query_failed"), |row| {
            (row.1.clone(), row.2)
        });
    PortError {
        kind,
        reason_key: reason.into(),
        os_code: Some(code),
    }
}

pub(super) fn os(error: rustix::io::Errno) -> PortError {
    errno(error.raw_os_error())
}

/// Classifies `errno`; call immediately after the failed libc call.
pub(super) fn last() -> PortError {
    errno(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
}

pub(super) fn invalid() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "path.unresolvable".into(),
        os_code: None,
    }
}

pub(super) fn changed() -> PortError {
    PortError {
        kind: PortErrorKind::IdentityChanged,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

/// The value was not returned, or reading it could require content materialization.
pub(super) fn unavailable() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "os.unsupported".into(),
        os_code: None,
    }
}
