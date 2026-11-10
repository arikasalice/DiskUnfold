//! Stable scan-root identity verification for future F18 registration.
use super::{WindowsDirectory, open_verified_stable};
use du_core_contracts::ports::{PortError, PortErrorKind};
use std::path::Path;

/// Opens a stable local NTFS root and checks the scan's exact volume and file IDs.
///
/// The returned directory and its ancestors remain held without delete sharing.
/// This verifies the root only: it neither starts a watch nor proves that a
/// recursive watch can exclude nested mounts or reparse points. Callers must
/// hold the returned value for the duration of any dependent operation.
pub fn open_matching_scan_root(
    path: &Path,
    expected_volume: &[u8],
    expected_file: &[u8],
) -> Result<WindowsDirectory, PortError> {
    if expected_volume.is_empty() || expected_file.is_empty() {
        return Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        });
    }
    let root = open_verified_stable(path)?;
    let actual_file = root.identity()?.file_id.to_le_bytes();
    if root.volume_guid.as_ref() != expected_volume || actual_file != expected_file {
        return Err(PortError {
            kind: PortErrorKind::IdentityChanged,
            reason_key: "os.query_failed".into(),
            os_code: None,
        });
    }
    Ok(root)
}

#[cfg(test)]
mod tests;
