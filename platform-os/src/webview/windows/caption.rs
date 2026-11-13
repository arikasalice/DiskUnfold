//! Native caption of the Windows window.
//!
//! The system draws the caption, so moving, double click to maximize, snap layouts, Alt+Space and
//! high contrast stay with Windows. The host only keeps the caption in step with the page: the
//! title follows `document.title` and the colors follow the app theme the page is showing.
use std::ffi::c_void;
use webview2_com::{
    DocumentTitleChangedEventHandler, Microsoft::Web::WebView2::Win32::ICoreWebView2, take_pwstr,
};
use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT, DWMWA_TEXT_COLOR,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE, DwmSetWindowAttribute,
        },
        System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            WindowsAndMessaging::{
                SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetWindowTextW,
                SystemParametersInfoW, WM_SETTINGCHANGE, WM_THEMECHANGED,
            },
        },
    },
    core::{BOOL, HSTRING, PWSTR, w},
};

/// Longest window title the caption shows, in UTF-16 units.
const MAX_TITLE: usize = 256;

/// Colors of one app theme as 0xRRGGBB. They repeat `--du-page`, `--du-text` and `--du-border`
/// from `gui-web/src/theme/tokens.css`; a test keeps the two in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Palette {
    pub background: u32,
    pub text: u32,
    pub border: u32,
}

pub(super) const LIGHT: Palette = Palette {
    background: 0xf5f5f6,
    text: 0x242528,
    border: 0xd9dce1,
};

pub(super) const DARK: Palette = Palette {
    background: 0x18191c,
    text: 0xf1f2f4,
    border: 0x41454d,
};

/// The Win32 COLORREF (0x00BBGGRR) for a 0xRRGGBB color.
pub(super) fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff)
}

/// The caption title for a page title: whole, single line and bounded.
pub(super) fn caption_title(page_title: &str) -> String {
    let single_line: String = page_title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut units = 0;
    single_line
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= MAX_TITLE
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Applies the app theme to the caption: dark or light, and the page colors. High contrast
/// keeps the colors the system chose.
pub(super) fn apply(window: HWND) {
    let contrast = high_contrast();
    let dark = !contrast && apps_use_dark_theme();
    let palette = if dark { DARK } else { LIGHT };
    set(
        window,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        BOOL::from(dark).0 as u32,
    );
    for (attribute, rgb) in [
        (DWMWA_CAPTION_COLOR, palette.background),
        (DWMWA_TEXT_COLOR, palette.text),
        (DWMWA_BORDER_COLOR, palette.border),
    ] {
        set(
            window,
            attribute,
            if contrast {
                DWMWA_COLOR_DEFAULT
            } else {
                colorref(rgb)
            },
        );
    }
}

/// Reapplies the theme when the system reports a color change.
pub(super) fn on_message(window: *mut c_void, kind: u32) {
    if kind == WM_SETTINGCHANGE || kind == WM_THEMECHANGED {
        apply(HWND(window));
    }
}

/// Keeps the caption title equal to the page title. Attributes the older systems do not know
/// are ignored by DWM.
pub(super) fn follow_title(view: &ICoreWebView2, window: HWND) -> windows::core::Result<()> {
    let mut token = 0;
    // SAFETY: The handler runs on the view's STA, where the window lives.
    unsafe {
        view.add_DocumentTitleChanged(
            &DocumentTitleChangedEventHandler::create(Box::new(move |view, _| {
                if let Some(view) = view {
                    let mut title = PWSTR::null();
                    view.DocumentTitle(&mut title)?;
                    let title = caption_title(&take_pwstr(title));
                    if !title.is_empty() {
                        let _ = SetWindowTextW(window, &HSTRING::from(title));
                    }
                }
                Ok(())
            })),
            &mut token,
        )
    }
}

fn set(window: HWND, attribute: DWMWINDOWATTRIBUTE, value: u32) {
    // SAFETY: Every attribute used here is one 32-bit value that outlives the call.
    unsafe {
        let _ = DwmSetWindowAttribute(
            window,
            attribute,
            std::ptr::from_ref(&value).cast::<c_void>(),
            std::mem::size_of::<u32>() as u32,
        );
    }
}

fn high_contrast() -> bool {
    let mut info = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a HIGHCONTRASTW whose size is declared.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            Some(std::ptr::from_mut(&mut info).cast::<c_void>()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && info.dwFlags.contains(HCF_HIGHCONTRASTON)
}

fn apps_use_dark_theme() -> bool {
    let mut value = 1_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: The buffer and its size describe one DWORD.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(std::ptr::from_mut(&mut value).cast::<c_void>()),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 0
}

#[cfg(test)]
mod tests;
