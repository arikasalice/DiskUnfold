//! WebView event delivery and native window-close coordination.

use std::sync::Arc;

use du_platform_os::webview::{EventCallbacks, Events};

struct DesktopEventCallbacks {
    next: Box<dyn Fn() -> Option<String> + Send + Sync>,
    post_failed: Box<dyn Fn(du_platform_os::webview::EventPostFailure) + Send + Sync>,
    document_started: Box<dyn Fn() + Send + Sync>,
    document_ready: Box<dyn Fn() + Send + Sync>,
    document_closed: Box<dyn Fn() + Send + Sync>,
}

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

/// Builds the event source and installs the shutdown ticker used by native close hooks.
pub(crate) fn event_source(
    events: Arc<du_engine_api::desktop::DesktopEvents>,
    shutdown: Arc<super::host::ShutdownController<super::host::HostShutdownPorts>>,
) -> Events {
    let request_shutdown = shutdown.clone();
    let pump_shutdown = shutdown.clone();
    DesktopEventCallbacks::new(events)
        .into_event_source()
        .with_shutdown_callbacks(
            move || {
                let request = du_core_contracts::shutdown_ipc::AppQuitRequest {
                    mode: du_core_contracts::shutdown_ipc::AppQuitMode::Normal,
                    confirm_unfinished_changes: None,
                };
                let _ = super::host::QuitHandler::request_quit(request_shutdown.as_ref(), &request);
            },
            move || {
                let _ = pump_shutdown.pump(super::host::current_time_ms());
            },
            move || shutdown.should_exit(),
        )
}

#[cfg(all(test, windows))]
mod tests;
