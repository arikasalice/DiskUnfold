//! Volume capacity re-observed through the collector's verified directory handle.
use super::{
    Anchor,
    errors::{errno, invalid},
    metadata, scope,
};
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{VolumeSpace, VolumeSpaceReader},
};
use rustix::{
    fd::{AsFd, OwnedFd},
    fs::fstatvfs,
};
use std::sync::Arc;

/// Reads whole-volume capacity, irrespective of the reserved-block policy.
pub(super) struct Reader {
    fd: Arc<OwnedFd>,
    device: u64,
    volume_id: Arc<[u8]>,
}

impl Reader {
    pub(super) fn new(anchor: &Anchor) -> Self {
        Self {
            fd: Arc::clone(&anchor.fd),
            device: anchor.device,
            volume_id: Arc::clone(&anchor.volume_id),
        }
    }

    fn identity(&self) -> Result<Vec<u8>, PortError> {
        let facts = metadata::handle(&*self.fd)?;
        if facts.device != self.device {
            return Err(changed());
        }
        let current = scope::volume_id(&*self.fd, facts.device);
        if *current != *self.volume_id {
            return Err(changed());
        }
        Ok(current)
    }
}

impl VolumeSpaceReader for Reader {
    /// Reads space only while the handle still identifies `expected_volume`.
    ///
    /// Every failure keeps its port kind with the volume-report reason key.
    fn read(&self, expected_volume: &[u8]) -> Result<VolumeSpace, PortError> {
        checked(expected_volume, || self.identity(), || space(&*self.fd)).map_err(|error| {
            PortError {
                reason_key: "space.volume_report_unavailable".into(),
                ..error
            }
        })
    }
}

fn changed() -> PortError {
    PortError {
        kind: PortErrorKind::IdentityChanged,
        reason_key: "scan.disconnected".into(),
        os_code: None,
    }
}

/// Discards a reading unless the identity matches before and after it.
fn checked<T>(
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

fn space(fd: impl AsFd) -> Result<VolumeSpace, PortError> {
    let value = fstatvfs(fd).map_err(errno)?;
    if value.f_frsize == 0 || value.f_bfree > value.f_blocks {
        return Err(invalid());
    }
    let total = value
        .f_blocks
        .checked_mul(value.f_frsize)
        .ok_or_else(invalid)?;
    let free = value
        .f_bfree
        .checked_mul(value.f_frsize)
        .ok_or_else(invalid)?;
    Ok(VolumeSpace {
        total_bytes: total,
        used_bytes: total - free,
        free_bytes: free,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn checks_identity_before_and_after_the_read() {
        let reads = Cell::new(0);
        let wrong = checked(
            b"expected",
            || Ok(b"other".to_vec()),
            || {
                reads.set(reads.get() + 1);
                Ok(())
            },
        );
        assert_eq!(
            wrong.err().map(|e| e.kind),
            Some(PortErrorKind::IdentityChanged)
        );
        assert_eq!(reads.get(), 0);
        let calls = Cell::new(0);
        let rebound = checked(
            b"expected",
            || {
                calls.set(calls.get() + 1);
                Ok(if calls.get() == 1 {
                    b"expected".to_vec()
                } else {
                    b"other".to_vec()
                })
            },
            || Ok(1),
        );
        assert_eq!(
            rebound.err().map(|e| e.kind),
            Some(PortErrorKind::IdentityChanged)
        );
    }
}
