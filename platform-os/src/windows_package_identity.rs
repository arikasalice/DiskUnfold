//! Detects whether the process runs with Windows package (MSIX) identity.
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

const APPMODEL_ERROR_NO_PACKAGE: u32 = 15_700;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

/// Returns `Some(true)` for a packaged process, `Some(false)` for an unpackaged
/// one, and `None` when the operating system gave any other answer.
pub fn has_package_identity() -> Option<bool> {
    let mut length = 0_u32;
    // SAFETY: `length` is a live u32. Passing a zero length and no buffer is the
    // documented size query; the call writes only the required length back.
    let status = unsafe { GetCurrentPackageFullName(&mut length, None) };
    match status.0 {
        APPMODEL_ERROR_NO_PACKAGE => Some(false),
        ERROR_INSUFFICIENT_BUFFER => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unpackaged_test_process_has_no_package_identity() {
        assert_eq!(has_package_identity(), Some(false));
    }
}
