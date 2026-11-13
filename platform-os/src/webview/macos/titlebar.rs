//! Transparent, full-size title bar of the macOS window.
//!
//! The window keeps its native close, minimize and zoom buttons, but the title bar is
//! transparent and the web content extends under it. The page draws its own bar there
//! (`gui-web` `TitleBar`). The host measures the space the buttons occupy and publishes it as
//! the CSS properties `--du-titlebar-inset-start` and `--du-titlebar-height`, so the page
//! never hard-codes where the buttons end.
use objc2_app_kit::{NSWindow, NSWindowButton, NSWindowStyleMask, NSWindowTitleVisibility};

/// Space between the last button and the first piece of page content, in points.
const BUTTON_GAP: f64 = 12.0;

/// Used when the system provides no zoom button to measure.
const FALLBACK_INSET: f64 = 80.0;

/// Style mask bit that lets the content extend under the title bar.
pub(super) fn style_mask() -> NSWindowStyleMask {
    NSWindowStyleMask::FullSizeContentView
}

/// Makes the title bar transparent and hides the native title text.
pub(super) fn configure(window: &NSWindow) {
    window.setTitlebarAppearsTransparent(true);
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
}

/// What the page needs to lay out its own bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Metrics {
    /// Width taken by the native buttons plus a gap, in points.
    pub inset_start: f64,
    /// Height of the native title bar area, in points. Zero in full screen, where there is none.
    pub height: f64,
}

/// Measures the buttons and the title bar of `window`.
pub(super) fn metrics(window: &NSWindow) -> Metrics {
    if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return Metrics {
            inset_start: 0.0,
            height: 0.0,
        };
    }
    let inset_start = window
        .standardWindowButton(NSWindowButton::ZoomButton)
        .map_or(FALLBACK_INSET, |button| {
            // The frame is in the title bar container's coordinates; the window's are needed.
            let in_window = button.convertRect_toView(button.bounds(), None);
            in_window.origin.x + in_window.size.width + BUTTON_GAP
        });
    let height = (window.frame().size.height - window.contentLayoutRect().size.height).max(0.0);
    Metrics {
        inset_start,
        height,
    }
}

/// JavaScript that publishes `metrics` as CSS custom properties on the document element.
pub(super) fn publish_script(metrics: Metrics) -> String {
    format!(
        "(function(){{var s=document.documentElement.style;s.setProperty('--du-titlebar-inset-start','{}px');s.setProperty('--du-titlebar-height','{}px');}})();",
        metrics.inset_start.round(),
        metrics.height.round()
    )
}

/// JavaScript that marks the app document as having the custom bar. The page reads the marker
/// as a read-only property and also needs the window message handler to draw the bar.
pub(super) fn marker_script(metrics: Metrics) -> String {
    format!(
        "if (window === window.top && location.href === 'diskunfold://app/index.html') {{ Object.defineProperty(window, '__diskunfoldTitlebar', {{ value: true }}); {} }}",
        publish_script(metrics)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_publish_script_sets_both_properties_in_whole_points() {
        let script = publish_script(Metrics {
            inset_start: 77.6,
            height: 28.2,
        });
        assert!(script.contains("--du-titlebar-inset-start','78px'"));
        assert!(script.contains("--du-titlebar-height','28px'"));
    }

    #[test]
    fn the_marker_is_defined_only_for_the_app_document() {
        let script = marker_script(Metrics {
            inset_start: 0.0,
            height: 0.0,
        });
        assert!(script.contains("location.href === 'diskunfold://app/index.html'"));
        assert!(script.contains("__diskunfoldTitlebar"));
        assert!(script.contains("window === window.top"));
    }
}
