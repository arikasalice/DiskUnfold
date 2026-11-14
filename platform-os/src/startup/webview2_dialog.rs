//! Text and flow of the pre-window dialog shown when the WebView2 Runtime is missing.
//!
//! The decision is pure so it is testable on any host: the Windows adapter supplies a
//! [`DialogUi`] backed by the native message box and the default browser. The dialog links to
//! Microsoft's public WebView2 page only; it never links to a direct download.
use super::StartupDialogOutcome;

/// Microsoft's public WebView2 page, opened in the default browser on request.
pub(super) const WEBVIEW2_PAGE_URL: &str =
    "https://developer.microsoft.com/microsoft-edge/webview2/";

/// The reason key the startup check puts in the error when no runtime is installed.
const MISSING_RUNTIME_KEY: &str = "startup.webview_missing";

/// `LANG_KOREAN`, the primary language identifier of the user's UI language.
const LANG_KOREAN: u16 = 0x12;

/// Languages the dialog is written in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DialogLanguage {
    Korean,
    English,
}

impl DialogLanguage {
    /// Picks the language from a Windows language identifier (`LANGID`); anything but Korean
    /// shows English.
    pub(super) fn from_lang_id(lang_id: u16) -> Self {
        if lang_id & 0x03ff == LANG_KOREAN {
            Self::Korean
        } else {
            Self::English
        }
    }
}

/// Title and body of the dialog, and the page its "Yes" button opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DialogText {
    pub(super) title: &'static str,
    pub(super) body: &'static str,
    pub(super) url: &'static str,
}

/// Whether the startup error says the WebView2 Runtime is missing.
pub(super) fn is_missing_runtime_error(detail: &str) -> bool {
    detail.contains(MISSING_RUNTIME_KEY)
}

/// The dialog text for the missing runtime in the given language.
pub(super) fn missing_runtime_text(language: DialogLanguage) -> DialogText {
    match language {
        DialogLanguage::Korean => DialogText {
            title: "DiskUnfold를 시작할 수 없습니다",
            body: "DiskUnfold를 실행하려면 Microsoft Edge WebView2 Runtime이 필요합니다. Windows 11에는 기본으로 포함되어 있지만 이 컴퓨터에서는 찾을 수 없습니다.\n\nMicrosoft WebView2 안내 페이지를 열어 설치한 뒤 DiskUnfold를 다시 실행하세요. 지금 페이지를 열까요?\n\n예: 안내 페이지 열기 / 아니요: 닫기",
            url: WEBVIEW2_PAGE_URL,
        },
        DialogLanguage::English => DialogText {
            title: "DiskUnfold could not start",
            body: "DiskUnfold needs the Microsoft Edge WebView2 Runtime. Windows 11 includes it, but it was not found on this computer.\n\nOpen Microsoft's WebView2 page, install the runtime, then start DiskUnfold again. Open the page now?\n\nYes: open the page / No: close",
            url: WEBVIEW2_PAGE_URL,
        },
    }
}

/// What the native adapter must provide.
pub(super) trait DialogUi {
    /// Shows a blocking Yes/No question. `Some(true)` is Yes, `Some(false)` is No, and `None` means
    /// the dialog could not be shown.
    fn ask_yes_no(&self, title: &str, body: &str) -> Option<bool>;
    /// Opens `url` in the default browser; returns whether the request was accepted.
    fn open_in_browser(&self, url: &str) -> bool;
}

/// Shows the missing-runtime dialog and opens the page when the user asks. Returns `None` when
/// `detail` is not the missing-runtime error, so the caller shows its ordinary error dialog.
pub(super) fn show_missing_runtime_dialog(
    ui: &dyn DialogUi,
    language: DialogLanguage,
    detail: &str,
) -> Option<StartupDialogOutcome> {
    if !is_missing_runtime_error(detail) {
        return None;
    }
    let text = missing_runtime_text(language);
    Some(match ui.ask_yes_no(text.title, text.body) {
        Some(open) => {
            if open {
                let _ = ui.open_in_browser(text.url);
            }
            StartupDialogOutcome::Acknowledged
        }
        None => StartupDialogOutcome::Failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct FakeUi {
        answer: Option<bool>,
        asked: Cell<usize>,
        opened: RefCell<Vec<String>>,
        last_body: RefCell<String>,
    }

    impl FakeUi {
        fn answering(answer: Option<bool>) -> Self {
            Self {
                answer,
                asked: Cell::new(0),
                opened: RefCell::new(Vec::new()),
                last_body: RefCell::new(String::new()),
            }
        }
    }

    impl DialogUi for FakeUi {
        fn ask_yes_no(&self, _title: &str, body: &str) -> Option<bool> {
            self.asked.set(self.asked.get() + 1);
            *self.last_body.borrow_mut() = body.to_owned();
            self.answer
        }
        fn open_in_browser(&self, url: &str) -> bool {
            self.opened.borrow_mut().push(url.to_owned());
            true
        }
    }

    #[test]
    fn korean_ui_languages_select_korean_and_others_english() {
        assert_eq!(DialogLanguage::from_lang_id(0x0412), DialogLanguage::Korean);
        assert_eq!(
            DialogLanguage::from_lang_id(0x0409),
            DialogLanguage::English
        );
        assert_eq!(
            DialogLanguage::from_lang_id(0x0411),
            DialogLanguage::English
        );
        assert_eq!(DialogLanguage::from_lang_id(0), DialogLanguage::English);
    }

    #[test]
    fn both_texts_name_the_runtime_and_use_the_public_page_only() {
        for language in [DialogLanguage::Korean, DialogLanguage::English] {
            let text = missing_runtime_text(language);
            assert!(
                text.body.contains("Microsoft Edge WebView2 Runtime"),
                "{language:?}"
            );
            assert_eq!(
                text.url,
                "https://developer.microsoft.com/microsoft-edge/webview2/"
            );
            let lower = text.url.to_ascii_lowercase();
            assert!(!lower.contains("fwlink") && !lower.contains("go.microsoft.com"));
            assert!(
                !text.body.contains("http"),
                "the URL is opened, not printed"
            );
        }
        assert!(
            missing_runtime_text(DialogLanguage::Korean)
                .body
                .contains("필요합니다")
        );
        assert!(
            missing_runtime_text(DialogLanguage::English)
                .body
                .contains("needs")
        );
    }

    #[test]
    fn a_missing_runtime_dialog_opens_the_page_on_yes() {
        let ui = FakeUi::answering(Some(true));
        let outcome =
            show_missing_runtime_dialog(&ui, DialogLanguage::Korean, "startup.webview_missing");
        assert_eq!(outcome, Some(StartupDialogOutcome::Acknowledged));
        assert_eq!(ui.asked.get(), 1);
        assert!(ui.last_body.borrow().contains("WebView2"));
        assert_eq!(*ui.opened.borrow(), vec![WEBVIEW2_PAGE_URL.to_owned()]);
    }

    #[test]
    fn answering_no_closes_without_opening_anything() {
        let ui = FakeUi::answering(Some(false));
        let outcome =
            show_missing_runtime_dialog(&ui, DialogLanguage::English, "startup.webview_missing");
        assert_eq!(outcome, Some(StartupDialogOutcome::Acknowledged));
        assert!(ui.opened.borrow().is_empty());
    }

    #[test]
    fn an_undisplayable_dialog_is_reported_as_failed() {
        let ui = FakeUi::answering(None);
        let outcome =
            show_missing_runtime_dialog(&ui, DialogLanguage::English, "startup.webview_missing");
        assert_eq!(outcome, Some(StartupDialogOutcome::Failed));
        assert!(ui.opened.borrow().is_empty());
    }

    #[test]
    fn other_startup_errors_keep_the_ordinary_dialog() {
        let ui = FakeUi::answering(Some(true));
        assert_eq!(
            show_missing_runtime_dialog(&ui, DialogLanguage::English, "startup.resource_integrity"),
            None
        );
        assert_eq!(ui.asked.get(), 0);
    }
}
