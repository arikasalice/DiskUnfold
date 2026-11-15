//! Process-wide view of Cloud Files placeholders.
use std::ffi::{CStr, c_void};

/// `PHCM_DISGUISE_PLACEHOLDER` from `ntifs.h`.
const DISGUISE_PLACEHOLDER: i8 = 1;
const NTDLL: [u16; 10] = [
    b'n' as u16,
    b't' as u16,
    b'd' as u16,
    b'l' as u16,
    b'l' as u16,
    b'.' as u16,
    b'd' as u16,
    b'l' as u16,
    b'l' as u16,
    0,
];
const SET_MODE: &CStr = c"RtlSetProcessPlaceholderCompatibilityMode";

/// `CHAR RtlSetProcessPlaceholderCompatibilityMode(CHAR Mode)` from `ntifs.h`.
type SetMode = unsafe extern "system" fn(i8) -> i8;
type Export = unsafe extern "system" fn() -> isize;

/// Why the disguised placeholder view was not established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceholderModeError {
    /// This Windows version does not export the mode (before Windows 10, version 1803).
    Unavailable,
    /// Windows refused the request with this negative `PHCM_ERROR_*` value.
    Refused(i8),
}

/// Asks Windows to present Cloud Files placeholders to this process as
/// ordinary files and directories (`PHCM_DISGUISE_PLACEHOLDER`).
///
/// Call this once at process start, before any thread observes the file
/// system: the mode is process-wide and decides whether handles opened later
/// report the cloud reparse point. It does not hydrate or change any file.
///
/// A failure leaves the mode chosen by Windows. Observation stays safe in
/// that case: a placeholder whose handle still reports a reparse point is
/// recorded as a link or an unverified entry and is never entered, and the
/// recall checks do not depend on the mode.
pub fn disguise_placeholders() -> Result<(), PlaceholderModeError> {
    let set_mode = set_mode_export().ok_or(PlaceholderModeError::Unavailable)?;
    // SAFETY: the export takes the mode by value, updates only this process's
    // compatibility state and neither retains nor dereferences caller memory.
    let previous = unsafe { set_mode(DISGUISE_PLACEHOLDER) };
    if previous < 0 {
        Err(PlaceholderModeError::Refused(previous))
    } else {
        Ok(())
    }
}

/// Resolves the export at run time so that the executable still starts on
/// Windows versions whose `ntdll.dll` lacks it.
fn set_mode_export() -> Option<SetMode> {
    // SAFETY: both names are NUL-terminated constants. `ntdll.dll` is mapped
    // into every process for its whole lifetime, so the lookup by its bare
    // name loads no module and the returned address never becomes dangling.
    let export = unsafe {
        let ntdll = GetModuleHandleW(NTDLL.as_ptr());
        if ntdll.is_null() {
            return None;
        }
        GetProcAddress(ntdll, SET_MODE.as_ptr().cast())?
    };
    // SAFETY: `ntifs.h` declares this export with exactly the `SetMode`
    // signature and the system calling convention.
    Some(unsafe { std::mem::transmute::<Export, SetMode>(export) })
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> Option<Export>;
}
