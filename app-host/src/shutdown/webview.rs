//! WebView event delivery and native window-close coordination.

use std::sync::Arc;

use du_platform_os::webview::ShutdownCallbacks;
#[cfg(any(windows, target_os = "linux"))]
use du_platform_os::webview::{EventCallbacks, Events};

use super::host::{HostShutdownPorts, ShutdownController, current_time_ms};

#[cfg(any(windows, target_os = "linux"))]
struct DesktopEventCallbacks {
    next: Box<dyn Fn() -> Option<String> + Send + Sync>,
    post_failed: Box<dyn Fn(du_platform_os::webview::EventPostFailure) + Send + Sync>,
    document_started: Box<dyn Fn() + Send + Sync>,
    document_ready: Box<dyn Fn() + Send + Sync>,
    document_closed: Box<dyn Fn() + Send + Sync>,
}

#[cfg(any(windows, target_os = "linux"))]
impl DesktopEventCallbacks {
    fn new(events: Arc<du_engine_api::desktop::DesktopEvents>) -> Self {
        let next_events = events.clone();
        let failed_events = events.clone();
        let started_events = events.clone();
        let ready_events = events.clone();
        let closed_events = events;
        Self {
            next: Box::new(move || next_events.next_json()),
            post_failed: Box::new(move |failure| {
                failed_events.report_post_failure(failure.failed, failure.discarded);
            }),
            document_started: Box::new(move || started_events.begin_document_generation()),
            document_ready: Box::new(move || ready_events.mark_document_ready()),
            document_closed: Box::new(move || closed_events.close_document_generation()),
        }
    }

    fn into_event_source(self) -> Events {
        Events::new(
            self.next,
            EventCallbacks::new(
                self.post_failed,
                self.document_started,
                self.document_ready,
                self.document_closed,
            ),
        )
    }
}

/// Native-loop hooks shared by every platform adapter.
///
/// A window close or application quit asks for a normal exit and is cancelled by the
/// adapter until the coordinator completes. The ticker drives one bounded step per
/// tick; step failures are logged and bounded by the coordinator, never swallowed.
pub(crate) fn shutdown_callbacks(
    shutdown: Arc<ShutdownController<HostShutdownPorts>>,
) -> ShutdownCallbacks {
    let request_shutdown = shutdown.clone();
    let pump_shutdown = shutdown.clone();
    ShutdownCallbacks::new(
        move || request_shutdown.request_window_close(),
        move || {
            // The coordinator logs the error and counts it toward its retry bound.
            let _ = pump_shutdown.pump(current_time_ms());
        },
        move || shutdown.should_exit(),
    )
}

/// Builds the event source and installs the shutdown hooks used by native close requests.
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn event_source(
    events: Arc<du_engine_api::desktop::DesktopEvents>,
    shutdown: Arc<ShutdownController<HostShutdownPorts>>,
) -> Events {
    DesktopEventCallbacks::new(events)
        .into_event_source()
        .with_shutdown(shutdown_callbacks(shutdown))
}

#[cfg(all(test, windows))]
mod tests;
