//! Validate native asset and message requests before invoking app callbacks.
use std::sync::Arc;

pub(super) const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; font-src 'self'; connect-src 'none'; frame-src 'none'; frame-ancestors 'none'; form-action 'none'; base-uri 'none'";
const MAX_MESSAGE: usize = 1024 * 1024;
#[cfg(target_os = "windows")]
pub(super) const ORIGIN: &str = "https://diskunfold.invalid";
#[cfg(not(target_os = "windows"))]
pub(super) const ORIGIN: &str = "diskunfold://app";

#[derive(Clone, Copy)]
pub struct Asset {
    pub mime: &'static str,
    pub bytes: &'static [u8],
}

pub type Dispatch = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Non-blocking source of serialized application event envelopes.
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[derive(Clone)]
pub struct Events {
    next: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    callbacks: EventCallbacks,
    shutdown: Option<ShutdownCallbacks>,
}

/// Native-loop hooks for an orderly shutdown owned by the application host.
///
/// The adapter calls `request_close` for every native close request (window
/// close or application quit), `pump` on the UI thread about every 16 ms, and
/// `should_exit` after each of them. A native close request is cancelled until
/// `should_exit` returns true, so the process never leaves while the host's
/// shutdown is still running. All three run on the UI thread and must not block.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[derive(Clone)]
pub struct ShutdownCallbacks {
    request_close: Arc<dyn Fn() + Send + Sync>,
    pump: Arc<dyn Fn() + Send + Sync>,
    should_exit: Arc<dyn Fn() -> bool + Send + Sync>,
    is_closing: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl ShutdownCallbacks {
    /// Creates the close-request, periodic-progress and completion callbacks.
    pub fn new(
        request_close: impl Fn() + Send + Sync + 'static,
        pump: impl Fn() + Send + Sync + 'static,
        should_exit: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            request_close: Arc::new(request_close),
            pump: Arc::new(pump),
            should_exit: Arc::new(should_exit),
            is_closing: None,
        }
    }

    /// Adds the probe that tells whether a close request started the shutdown.
    ///
    /// A request that only asks the user to confirm leaves the shutdown idle. The
    /// macOS adapter uses the probe to decide whether a postponed quit has anything
    /// that will answer it. Without a probe the shutdown is assumed to be running.
    pub fn with_closing_probe(
        mut self,
        is_closing: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        self.is_closing = Some(Arc::new(is_closing));
        self
    }

    pub(super) fn request_window_close(&self) {
        (self.request_close)();
    }

    pub(super) fn pump(&self) {
        (self.pump)();
    }

    pub(super) fn is_complete(&self) -> bool {
        (self.should_exit)()
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn is_closing(&self) -> bool {
        self.is_closing.as_ref().is_none_or(|probe| probe())
    }
}

/// Callbacks for delivery loss and top-level document lifecycle changes.
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[derive(Clone)]
pub struct EventCallbacks {
    post_failed: Arc<dyn Fn(EventPostFailure) + Send + Sync>,
    document_started: Arc<dyn Fn() + Send + Sync>,
    document_ready: Arc<dyn Fn() + Send + Sync>,
    document_closed: Arc<dyn Fn() + Send + Sync>,
}

/// Describes one failed native post and the later events discarded from its batch.
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[derive(Debug, PartialEq, Eq)]
pub struct EventPostFailure {
    /// Serialized envelope rejected by the native WebView post call.
    pub failed: String,
    /// Later envelopes in the same bounded batch that were discarded.
    pub discarded: Vec<String>,
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
impl Events {
    /// Creates the application event source with native delivery callbacks.
    pub fn new(
        next: impl Fn() -> Option<String> + Send + Sync + 'static,
        callbacks: EventCallbacks,
    ) -> Self {
        Self {
            next: Arc::new(next),
            callbacks,
            shutdown: None,
        }
    }

    /// Adds host shutdown callbacks built once and shared with other adapters.
    pub fn with_shutdown(mut self, callbacks: ShutdownCallbacks) -> Self {
        self.shutdown = Some(callbacks);
        self
    }

    /// Adds app-host shutdown callbacks for native close requests and event-loop polling.
    pub fn with_shutdown_callbacks(
        mut self,
        request_close: impl Fn() + Send + Sync + 'static,
        pump: impl Fn() + Send + Sync + 'static,
        should_exit: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        self.shutdown = Some(ShutdownCallbacks::new(request_close, pump, should_exit));
        self
    }

    pub(super) fn next(&self) -> Option<String> {
        (self.next)()
    }

    pub(super) fn report_post_failure(&self, failure: EventPostFailure) {
        (self.callbacks.post_failed)(failure);
    }

    pub(super) fn document_started(&self) {
        (self.callbacks.document_started)();
    }

    pub(super) fn document_ready(&self) {
        (self.callbacks.document_ready)();
    }

    pub(super) fn document_closed(&self) {
        (self.callbacks.document_closed)();
    }

    pub(super) fn request_window_close(&self) {
        if let Some(shutdown) = &self.shutdown {
            shutdown.request_window_close();
        }
    }

    pub(super) fn pump_shutdown(&self) {
        if let Some(shutdown) = &self.shutdown {
            shutdown.pump();
        }
    }

    pub(super) fn shutdown_is_complete(&self) -> bool {
        self.shutdown
            .as_ref()
            .is_some_and(ShutdownCallbacks::is_complete)
    }

    pub(super) fn has_shutdown_callbacks(&self) -> bool {
        self.shutdown.is_some()
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
impl EventCallbacks {
    /// Creates failure reporting and document lifecycle callbacks.
    pub fn new(
        post_failed: impl Fn(EventPostFailure) + Send + Sync + 'static,
        document_started: impl Fn() + Send + Sync + 'static,
        document_ready: impl Fn() + Send + Sync + 'static,
        document_closed: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            post_failed: Arc::new(post_failed),
            document_started: Arc::new(document_started),
            document_ready: Arc::new(document_ready),
            document_closed: Arc::new(document_closed),
        }
    }
}

/// Outcome of attempting to post one IPC response to the WebView.
#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseDelivery {
    Posted,
    NotPosted,
}

/// One-shot notification for an IPC reply that was posted or discarded.
///
/// This can run on the WebView STA or a worker during teardown. It must not wait
/// or perform I/O.
#[cfg(target_os = "windows")]
pub type ResponseDeliveryCallback = Box<dyn FnOnce(ResponseDelivery) + Send>;

/// Runs on the IPC worker with the request and response, before the reply is queued.
/// Return a callback only for responses that need a delivery notification.
#[cfg(target_os = "windows")]
pub type ResponseDeliveryObserver =
    Arc<dyn Fn(&str, &str) -> Option<ResponseDeliveryCallback> + Send + Sync>;

/// Windows dispatch lane selected by the application after parsing the request.
#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchLane {
    Ordered,
    Volume,
    QueryControl,
    /// FIFO delivery for pause, resume and cancellation of admitted jobs.
    JobControl,
}

pub struct WebApp {
    pub asset: fn(&str) -> Option<Asset>,
    pub dispatch: Dispatch,
    /// Exposes the Mac locale bridge marker only after locale queries are registered.
    #[cfg(target_os = "macos")]
    pub locale_enabled: bool,
    /// Display language tag (for example `ko`) that selects the language of native alerts.
    #[cfg(target_os = "macos")]
    pub native_language: String,
    /// Selects a bounded Windows queue; the dispatch callback still validates every request.
    #[cfg(target_os = "windows")]
    pub route: fn(&str) -> DispatchLane,
    /// Optionally prepares a one-shot notification for each IPC reply on its worker.
    #[cfg(target_os = "windows")]
    pub response_delivery: Option<ResponseDeliveryObserver>,
    /// Optional application-owned script installed before navigating to the entry document.
    #[cfg(target_os = "windows")]
    pub initialization_script: Option<&'static str>,
    pub icon: &'static [u8],
}

pub(super) fn entry() -> String {
    format!("{ORIGIN}/index.html")
}

/// Response header fields for one asset, with the media type listed explicitly.
///
/// A custom scheme handler may replace the content type it set on the response when it later
/// installs a header set, and `nosniff` then makes WebKit discard a stylesheet that arrives
/// without a type. The Content-Type field therefore always travels with the other fields.
#[cfg(any(target_os = "linux", test))]
pub(super) fn response_headers(mime: &str) -> [(&'static str, String); 3] {
    [
        ("Content-Type", mime.to_owned()),
        ("Content-Security-Policy", CSP.to_owned()),
        ("X-Content-Type-Options", "nosniff".to_owned()),
    ]
}

pub(super) fn resource(asset: fn(&str) -> Option<Asset>, method: &str, url: &str) -> Option<Asset> {
    let path = url.strip_prefix(ORIGIN)?;
    if method != "GET"
        || !path.starts_with('/')
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._/".contains(&b))
        || path[1..]
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    asset(path)
}

pub(super) fn message(
    dispatch: &Dispatch,
    from_main_frame: bool,
    url: &str,
    data: &str,
) -> Option<String> {
    if !from_main_frame || !trusted_message(url, data) {
        return None;
    }
    dispatch(data)
}

pub(super) fn trusted_message(url: &str, data: &str) -> bool {
    url == entry() && data.len() <= MAX_MESSAGE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(path: &str) -> Option<Asset> {
        (path == "/index.html").then_some(Asset {
            mime: "text/html",
            bytes: b"ok",
        })
    }

    #[test]
    fn resources_are_exact_and_local() {
        assert!(resource(asset, "GET", &entry()).is_some());
        for path in [
            "/../index.html",
            "/%69ndex.html",
            "/index.html?x",
            ".evil/index.html",
            "//index.html",
        ] {
            assert!(resource(asset, "GET", &format!("{ORIGIN}{path}")).is_none());
        }
        assert!(resource(asset, "POST", &entry()).is_none());
    }

    #[test]
    fn every_asset_response_names_its_media_type_next_to_nosniff() {
        let headers = response_headers("text/css; charset=utf-8");
        assert_eq!(
            headers[0],
            ("Content-Type", "text/css; charset=utf-8".to_owned())
        );
        assert!(
            headers
                .iter()
                .any(|(name, value)| { *name == "X-Content-Type-Options" && value == "nosniff" })
        );
        assert!(
            headers
                .iter()
                .any(|(name, value)| { *name == "Content-Security-Policy" && value == CSP })
        );
    }

    #[test]
    fn ipc_requires_the_trusted_top_level_document() {
        let dispatch: Dispatch = Arc::new(|data| Some(data.to_owned()));
        assert_eq!(
            message(&dispatch, true, &entry(), "ok").as_deref(),
            Some("ok")
        );
        assert!(message(&dispatch, false, &entry(), "no").is_none());
        assert!(message(&dispatch, true, "https://example.invalid/", "no").is_none());
        assert!(message(&dispatch, true, &entry(), &"x".repeat(MAX_MESSAGE + 1)).is_none());
    }
}
