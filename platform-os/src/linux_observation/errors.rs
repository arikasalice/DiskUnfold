//! Neutral classification of Linux error numbers. OS messages never leave this module.
use du_core_contracts::ports::{PortError, PortErrorKind};
use rustix::io::Errno;

/// Maps an error number to the neutral port classification and a reason key.
pub(super) fn errno(error: Errno) -> PortError {
    let (kind, reason_key) = match error {
        Errno::NOENT => (PortErrorKind::NotFound, "os.query_failed"),
        Errno::ACCESS | Errno::PERM => (PortErrorKind::PermissionDenied, "os.query_failed"),
        // Explicit reports that the device, medium or transport has gone away.
        Errno::NODEV | Errno::NOTCONN | Errno::NOMEDIUM | Errno::HOSTDOWN => {
            (PortErrorKind::Io, "scan.disconnected")
        }
        Errno::NOSYS | Errno::OPNOTSUPP => (PortErrorKind::Unsupported, "os.unsupported"),
        // A link, another mount or another kind where a directory was expected.
        Errno::LOOP | Errno::XDEV | Errno::NOTDIR | Errno::NAMETOOLONG => {
            (PortErrorKind::Unsupported, "path.unresolvable")
        }
        Errno::AGAIN => (PortErrorKind::Busy, "os.query_failed"),
        _ => (PortErrorKind::Io, "os.query_failed"),
    };
    PortError {
        kind,
        reason_key: reason_key.into(),
        os_code: Some(error.raw_os_error()),
    }
}

/// A path, entry or identity this adapter must not resolve or traverse.
pub(super) fn invalid() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "path.unresolvable".into(),
        os_code: None,
    }
}

/// A value the kernel did not return for this observation.
pub(super) fn unavailable() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "os.unsupported".into(),
        os_code: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_without_os_text() {
        let cases = [
            (Errno::NOENT, PortErrorKind::NotFound, "os.query_failed"),
            (
                Errno::ACCESS,
                PortErrorKind::PermissionDenied,
                "os.query_failed",
            ),
            (
                Errno::PERM,
                PortErrorKind::PermissionDenied,
                "os.query_failed",
            ),
            (Errno::NODEV, PortErrorKind::Io, "scan.disconnected"),
            (Errno::NOTCONN, PortErrorKind::Io, "scan.disconnected"),
            (Errno::IO, PortErrorKind::Io, "os.query_failed"),
            (Errno::LOOP, PortErrorKind::Unsupported, "path.unresolvable"),
            (Errno::XDEV, PortErrorKind::Unsupported, "path.unresolvable"),
            (
                Errno::NOTDIR,
                PortErrorKind::Unsupported,
                "path.unresolvable",
            ),
        ];
        for (error, kind, reason) in cases {
            let mapped = errno(error);
            assert_eq!((mapped.kind, mapped.reason_key.as_str()), (kind, reason));
            assert_eq!(mapped.os_code, Some(error.raw_os_error()));
        }
    }
}
