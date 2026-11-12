use super::super::super::contract::{DispatchLane, trusted_message};
use super::super::folder_drop::{self, Reason};
use super::pool::{EnqueueResult, FolderRequest, Request, Shared};
use super::post;
use std::sync::Arc;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;

impl RequestQueue {
    pub(super) fn new(
        shared: Arc<Shared>,
        generation: u64,
        route: fn(&str) -> DispatchLane,
    ) -> Self {
        Self {
            shared,
            generation,
            route,
        }
    }
}

/// A bounded queue for requests accepted from the trusted top-level document.
#[derive(Clone)]
pub(in crate::webview::windows) struct RequestQueue {
    shared: Arc<Shared>,
    generation: u64,
    route: fn(&str) -> DispatchLane,
}

impl RequestQueue {
    pub(in crate::webview::windows) fn submit(
        &self,
        view: &ICoreWebView2,
        origin: &str,
        id: &str,
        data: &str,
    ) -> windows::core::Result<()> {
        if !self.is_active() {
            return Ok(());
        }
        if !trusted_message(origin, data) {
            return post(view, &format!("{id}\n"));
        }
        match self.enqueue_ipc(origin, id, data) {
            EnqueueResult::Accepted | EnqueueResult::Stale | EnqueueResult::Closed => Ok(()),
            EnqueueResult::Full => post(view, &format!("{id}\n")),
        }
    }

    /// Schedules local directory verification off the WebView thread.
    pub(in crate::webview::windows) fn submit_folder(
        &self,
        view: &ICoreWebView2,
        id: u64,
        path: Vec<u16>,
    ) -> windows::core::Result<()> {
        if !self.is_active() {
            return Ok(());
        }
        match self.enqueue_folder(id, path) {
            EnqueueResult::Accepted | EnqueueResult::Stale | EnqueueResult::Closed => Ok(()),
            EnqueueResult::Full => {
                post(view, &folder_drop::reason_message(id, Reason::Unsupported))
            }
        }
    }

    pub(super) fn enqueue_ipc(&self, origin: &str, id: &str, data: &str) -> EnqueueResult {
        if !trusted_message(origin, data) {
            return EnqueueResult::Full;
        }
        self.shared.enqueue_ipc(
            Request {
                generation: self.generation,
                id: id.to_owned(),
                origin: origin.to_owned(),
                data: data.to_owned(),
            },
            (self.route)(data).into(),
        )
    }

    pub(super) fn enqueue_folder(&self, id: u64, path: Vec<u16>) -> EnqueueResult {
        self.shared.enqueue_folder(FolderRequest {
            generation: self.generation,
            id,
            path,
        })
    }

    pub(in crate::webview::windows) fn is_active(&self) -> bool {
        self.shared.is_active(self.generation)
    }
}
