//! GTK header bar that shows the app name and the screen the page is showing.
//!
//! The page keeps `document.title` as "DiskUnfold — <screen>" (macOS and Windows read the same
//! words in their own bars). The header bar draws the app name as its title and the screen as a
//! subtitle, and the window title carries the whole text for the task switcher.
use gtk4::prelude::*;
use webkit6::prelude::*;

const APP: &str = "DiskUnfold";
const SEPARATOR: &str = " \u{2014} ";

/// Splits a page title into the app name and the screen, if it names one.
pub(super) fn split_title(page_title: &str) -> (&str, Option<&str>) {
    match page_title
        .strip_prefix(APP)
        .and_then(|rest| rest.strip_prefix(SEPARATOR))
    {
        Some(screen) if !screen.trim().is_empty() => (APP, Some(screen.trim())),
        _ => (APP, None),
    }
}

/// Installs the header bar on `window` and keeps it in step with the title of `view`.
pub(super) fn install(window: &gtk4::Window, view: &webkit6::WebView) {
    let title = gtk4::Label::new(Some(APP));
    title.add_css_class("title");
    let subtitle = gtk4::Label::new(None);
    subtitle.add_css_class("subtitle");
    subtitle.set_visible(false);
    let words = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    words.set_valign(gtk4::Align::Center);
    words.append(&title);
    words.append(&subtitle);
    let header = gtk4::HeaderBar::new();
    header.set_title_widget(Some(&words));
    window.set_titlebar(Some(&header));
    let window = window.downgrade();
    view.connect_title_notify(move |view| {
        let Some(window) = window.upgrade() else {
            return;
        };
        let page = view.title().unwrap_or_default();
        let (_, screen) = split_title(page.as_str());
        subtitle.set_text(screen.unwrap_or_default());
        subtitle.set_visible(screen.is_some());
        window.set_title(Some(if page.is_empty() { APP } else { page.as_str() }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_title_with_a_screen_splits_into_name_and_subtitle() {
        assert_eq!(
            split_title("DiskUnfold — Startup and privacy"),
            ("DiskUnfold", Some("Startup and privacy"))
        );
    }

    #[test]
    fn other_titles_show_only_the_app_name() {
        assert_eq!(split_title("DiskUnfold"), ("DiskUnfold", None));
        assert_eq!(split_title("DiskUnfold — "), ("DiskUnfold", None));
        assert_eq!(split_title("Elsewhere — Screen"), ("DiskUnfold", None));
        assert_eq!(split_title(""), ("DiskUnfold", None));
    }
}
