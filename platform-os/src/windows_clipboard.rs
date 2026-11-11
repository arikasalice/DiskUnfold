//! Immediate, complete CF_UNICODETEXT writes on Windows.
//!
//! The clipboard is shared with other applications. A failed publication after
//! EmptyClipboard can leave it empty; Windows does not provide atomic rollback.
//! The caller must record success only when `write_text` returns `Ok`.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use du_core_contracts::ports::{ClipboardPort, PortError, PortErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardWriteError {
    InvalidText,
    Unavailable,
    System,
}

#[derive(Debug, Default)]
pub struct WindowsClipboard;

impl WindowsClipboard {
    /// Converts only paths that can be represented exactly as clipboard text.
    pub fn path_text(&self, path: &du_core_contracts::result::ResultNativePath) -> Option<String> {
        if path.encoding != du_core_contracts::PathEncoding::WindowsUtf16le {
            return None;
        }
        let bytes = path.raw.bytes().ok()?;
        let (chunks, remainder) = bytes.as_chunks::<2>();
        if !remainder.is_empty() {
            return None;
        }
        let units: Vec<u16> = chunks
            .iter()
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        let text = String::from_utf16(&units).ok()?;
        if text.contains('\0') {
            None
        } else {
            Some(text)
        }
    }

    /// Writes the whole UTF-16 value while holding the native clipboard open.
    fn write_native_text(&self, text: &str) -> Result<(), ClipboardWriteError> {
        let wide = encode_text(text)?;
        write_native(&wide)
    }
}

impl ClipboardPort for WindowsClipboard {
    fn can_write_text(&self) -> bool {
        true
    }

    fn write_text(&self, text: &str) -> Result<(), PortError> {
        self.write_native_text(text).map_err(|error| PortError {
            kind: match error {
                ClipboardWriteError::InvalidText => PortErrorKind::Corrupt,
                ClipboardWriteError::Unavailable => PortErrorKind::Busy,
                ClipboardWriteError::System => PortErrorKind::Io,
            },
            reason_key: "clipboard.failed".into(),
            os_code: None,
        })
    }
}

fn encode_text(text: &str) -> Result<Vec<u16>, ClipboardWriteError> {
    if text.contains('\0') {
        return Err(ClipboardWriteError::InvalidText);
    }
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    Ok(wide)
}

type Handle = *mut c_void;

#[link(name = "user32")]
unsafe extern "system" {
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: Handle,
    ) -> Handle;
    fn DestroyWindow(window: Handle) -> i32;
    fn OpenClipboard(window: Handle) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: Handle) -> Handle;
    fn CloseClipboard() -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    fn GlobalLock(memory: Handle) -> Handle;
    fn GlobalUnlock(memory: Handle) -> i32;
    fn GlobalFree(memory: Handle) -> Handle;
}

struct Window(Handle);
impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: this thread created the window and destroys it once.
        unsafe {
            DestroyWindow(self.0);
        }
    }
}
struct GlobalMemory(Handle);
impl Drop for GlobalMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: ownership remains ours unless SetClipboardData succeeds.
            unsafe {
                GlobalFree(self.0);
            }
        }
    }
}
struct OpenedClipboard;
impl Drop for OpenedClipboard {
    fn drop(&mut self) {
        // SAFETY: this guard exists only after a successful OpenClipboard.
        unsafe {
            CloseClipboard();
        }
    }
}

fn write_native(wide: &[u16]) -> Result<(), ClipboardWriteError> {
    let bytes = wide
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or(ClipboardWriteError::InvalidText)?;
    // SAFETY: the predefined STATIC class is NUL-terminated; HWND_MESSAGE creates
    // an invisible, thread-owned message-only window with no application callback.
    let window = unsafe {
        CreateWindowExW(
            0,
            [83, 84, 65, 84, 73, 67, 0].as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            -3isize as Handle,
            null_mut(),
            null_mut(),
            null_mut(),
        )
    };
    if window.is_null() {
        return Err(ClipboardWriteError::System);
    }
    let _window = Window(window);
    // SAFETY: allocation is movable as required by SetClipboardData; `bytes` is
    // nonzero and checked. The guard frees it unless ownership transfers.
    let mut memory = GlobalMemory(unsafe { GlobalAlloc(2, bytes) });
    if memory.0.is_null() {
        return Err(ClipboardWriteError::System);
    }
    // SAFETY: a successful lock yields a writable allocation of `bytes` bytes.
    let destination = unsafe { GlobalLock(memory.0) };
    if destination.is_null() {
        return Err(ClipboardWriteError::System);
    }
    // SAFETY: source and allocation do not overlap, and both contain `wide.len()` units.
    unsafe {
        std::ptr::copy_nonoverlapping(wide.as_ptr(), destination.cast::<u16>(), wide.len());
        GlobalUnlock(memory.0);
    }
    // SAFETY: window remains valid until after clipboard closure. Opening can fail
    // when another process has it open; do not block or retry on the UI thread.
    if unsafe { OpenClipboard(window) } == 0 {
        return Err(ClipboardWriteError::Unavailable);
    }
    let _opened = OpenedClipboard;
    // SAFETY: the clipboard is open and the owner window is non-null. Only one
    // complete prefilled format is published; on failure no partial text is set.
    if unsafe { EmptyClipboard() } == 0 {
        return Err(ClipboardWriteError::System);
    }
    if unsafe { SetClipboardData(13, memory.0) }.is_null() {
        return Err(ClipboardWriteError::System);
    }
    // The system owns the allocation after a successful SetClipboardData.
    memory.0 = null_mut();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encodes_complete_unicode_without_normalization() {
        assert_eq!(
            encode_text("한😀\r\nX"),
            Ok(vec![0xd55c, 0xd83d, 0xde00, 13, 10, 88, 0])
        );
        assert_eq!(encode_text(""), Ok(vec![0]));
    }
    #[test]
    fn path_conversion_requires_exact_windows_text() {
        use du_core_contracts::{PathBytes, PathEncoding, result::ResultNativePath};
        let make = |encoding, raw: &[u8]| ResultNativePath {
            encoding,
            raw: PathBytes::from_bytes(raw),
        };
        let clipboard = WindowsClipboard;
        assert_eq!(
            clipboard.path_text(&make(
                PathEncoding::WindowsUtf16le,
                &[65, 0, 0x3d, 0xd8, 0, 0xde]
            )),
            Some("A😀".into())
        );
        assert_eq!(
            clipboard.path_text(&make(PathEncoding::Unix, &[65, 0])),
            None
        );
        assert_eq!(
            clipboard.path_text(&make(PathEncoding::WindowsUtf16le, &[65])),
            None
        );
        assert_eq!(
            clipboard.path_text(&make(PathEncoding::WindowsUtf16le, &[0, 0xd8])),
            None
        );
        assert_eq!(
            clipboard.path_text(&make(PathEncoding::WindowsUtf16le, &[0, 0])),
            None
        );
    }
    #[test]
    fn rejects_embedded_nul_instead_of_copying_a_prefix() {
        assert_eq!(
            encode_text("first\0second"),
            Err(ClipboardWriteError::InvalidText)
        );
    }
}
