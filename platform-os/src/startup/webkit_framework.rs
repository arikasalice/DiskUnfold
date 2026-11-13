//! Reads the WebKit framework bundle version that backs `WKWebView`.
use objc2::ClassType;
use objc2_foundation::{NSBundle, NSString};
use objc2_web_kit::WKWebView;

/// The loaded WebKit framework's bundle version, such as `21622.1.22.11.15`.
///
/// The bundle is the one that defines `WKWebView`, so the value describes the
/// framework this process uses and not a path guessed from the OS layout.
pub(super) fn framework_version() -> Option<String> {
    // SAFETY: `WKWebView::class()` is a live class object, so `bundleForClass:`
    // returns the bundle that defines it; the call reads no caller memory.
    let bundle = unsafe { NSBundle::bundleForClass(WKWebView::class()) };
    ["CFBundleVersion", "CFBundleShortVersionString"]
        .into_iter()
        .find_map(|key| {
            let value = bundle.objectForInfoDictionaryKey(&NSString::from_str(key))?;
            let text = value.downcast::<NSString>().ok()?;
            Some(text.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::super::{WebViewAvailability, webview_runtime::linked_runtime_availability};

    #[test]
    fn the_loaded_webkit_framework_reports_a_usable_version() {
        let version = super::framework_version();
        assert_eq!(
            linked_runtime_availability(version.as_deref()),
            WebViewAvailability::Available,
            "{version:?}"
        );
    }
}
