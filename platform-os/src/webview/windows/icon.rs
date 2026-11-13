//! Load the launcher's embedded icon for the window caption and task switcher.

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            DestroyIcon, GetSystemMetrics, HICON, ICON_BIG, ICON_SMALL, IMAGE_FLAGS, IMAGE_ICON,
            LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SYSTEM_METRICS_INDEX,
            SendMessageW, WM_SETICON,
        },
    },
    core::PCWSTR,
};

// Matches the group icon ID in release-packaging/launcher/windows.rc.
const ICON_RESOURCE_ID: u16 = 1;

struct OwnedIcon(HICON);

impl OwnedIcon {
    fn load(
        module: HINSTANCE,
        width: SYSTEM_METRICS_INDEX,
        height: SYSTEM_METRICS_INDEX,
    ) -> windows::core::Result<Self> {
        // SAFETY: This is the executable's live module handle. A 16-bit value in
        // the pointer is the Win32 convention for an integer resource identifier.
        let icon = unsafe {
            LoadImageW(
                Some(module),
                PCWSTR(usize::from(ICON_RESOURCE_ID) as *const u16),
                IMAGE_ICON,
                GetSystemMetrics(width),
                GetSystemMetrics(height),
                IMAGE_FLAGS::default(),
            )?
        };
        Ok(Self(HICON(icon.0)))
    }
}

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        // SAFETY: LoadImageW returned an owned icon. The window has been destroyed.
        let _ = unsafe { DestroyIcon(self.0) };
    }
}

pub(super) struct WindowIcons {
    small: OwnedIcon,
    large: OwnedIcon,
}

impl WindowIcons {
    pub(super) fn load() -> windows::core::Result<Self> {
        // SAFETY: A null name retrieves the module of the current executable.
        let module = HINSTANCE(unsafe { GetModuleHandleW(PCWSTR::null())? }.0);
        Ok(Self {
            small: OwnedIcon::load(module, SM_CXSMICON, SM_CYSMICON)?,
            large: OwnedIcon::load(module, SM_CXICON, SM_CYICON)?,
        })
    }

    pub(super) fn install(&self, window: &windows_window::Window) {
        let window = HWND(window.hwnd());
        // SAFETY: The window and icons are used on their UI thread, and the caller
        // drops the window before dropping the icons it borrows.
        unsafe {
            SendMessageW(
                window,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(self.small.0.0 as isize)),
            );
            SendMessageW(
                window,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(self.large.0.0 as isize)),
            );
        }
    }
}
