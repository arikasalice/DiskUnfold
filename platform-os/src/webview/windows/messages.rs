//! Deliver WebView2 messages on the STA while application handlers use bounded lanes.
mod pool;
mod requests;
#[cfg(test)]
mod tests;

use pool::ControllerOwner;
#[cfg(test)]
use pool::EnqueueResult;
pub(in crate::webview::windows) use pool::WorkerPool;
pub(in crate::webview::windows) use requests::RequestQueue;

use super::super::contract::{DispatchLane, EventPostFailure, Events};
use super::reply::Reply;
use std::{cell::Cell, io, rc::Rc};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{KillTimer, SetTimer, WM_TIMER},
    },
    core::{HSTRING, PCWSTR},
};

const TIMER_ID: usize = 0x4455;
const QUEUE_LIMIT: usize = 32;

pub(super) struct Bridge {
    view: ICoreWebView2,
    ready: Rc<Cell<bool>>,
    owner: ControllerOwner,
    requests: RequestQueue,
    events: Option<Events>,
}

impl Bridge {
    pub(super) fn start(
        view: ICoreWebView2,
        route: fn(&str) -> DispatchLane,
        owner: ControllerOwner,
        events: Option<Events>,
        ready: Rc<Cell<bool>>,
    ) -> Self {
        let requests = owner.requests(route);
        Self {
            view,
            ready,
            owner,
            requests,
            events,
        }
    }

    pub(super) fn requests(&self) -> RequestQueue {
        self.requests.clone()
    }

    pub(super) fn flush(&self) {
        if !self.ready.get() || !self.owner.is_active() {
            return;
        }
        let active = deliver_reply_batch(&self.owner, self.owner.take_replies(), |wire| {
            post(&self.view, wire)
        });
        if !active {
            return;
        }
        if let Some(events) = &self.events {
            let _ = flush_events_if_active(&self.owner, events, |wire| post(&self.view, wire));
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.owner.revoke();
        if let Some(events) = &self.events {
            events.document_closed();
        }
    }
}

fn deliver_reply_batch<E>(
    owner: &ControllerOwner,
    replies: Vec<Reply>,
    mut post: impl FnMut(&str) -> Result<(), E>,
) -> bool {
    for reply in replies {
        if !owner.is_active() {
            drop(reply);
            continue;
        }
        let _ = reply.deliver_with_owner(|wire| post(wire), || owner.is_active());
    }
    owner.is_active()
}

fn flush_events_if_active<E>(
    owner: &ControllerOwner,
    events: &Events,
    post: impl FnMut(&str) -> Result<(), E>,
) -> bool {
    if !owner.is_active() {
        return false;
    }
    // This window has no controller replacement path; owner revocation follows the
    // message loop. A future replacement must make Events::next reservation-based so
    // a reentrant owner change cannot strand a batch already removed from F24.
    flush_event_batch(
        || events.next(),
        post,
        |failure| events.report_post_failure(failure),
    );
    true
}

fn flush_event_batch<E>(
    mut next: impl FnMut() -> Option<String>,
    mut post: impl FnMut(&str) -> Result<(), E>,
    mut on_post_failure: impl FnMut(EventPostFailure),
) {
    let batch: Vec<_> = std::iter::from_fn(&mut next).take(QUEUE_LIMIT).collect();
    for (index, payload) in batch.iter().enumerate() {
        if post(&format!("event\n{payload}")).is_err() {
            on_post_failure(EventPostFailure {
                failed: payload.clone(),
                discarded: batch[index + 1..].to_vec(),
            });
            break;
        }
    }
}

pub(super) fn post(view: &ICoreWebView2, data: &str) -> windows::core::Result<()> {
    let payload = HSTRING::from(data);
    // SAFETY: The view is used on its owning STA and WebView2 copies the string.
    unsafe { view.PostWebMessageAsString(PCWSTR(payload.as_ptr())) }
}

pub(super) fn is_tick(message: u32, parameter: usize) -> bool {
    message == WM_TIMER && parameter == TIMER_ID
}

pub(super) struct WindowTimer(HWND);

impl WindowTimer {
    pub(super) fn start(window: HWND) -> io::Result<Self> {
        // SAFETY: The window is live and owned by the calling UI thread.
        if unsafe { SetTimer(Some(window), TIMER_ID, 16, None) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(window))
    }
}

impl Drop for WindowTimer {
    fn drop(&mut self) {
        // SAFETY: The timer is dropped on the UI thread before the window is destroyed.
        let _ = unsafe { KillTimer(Some(self.0), TIMER_ID) };
    }
}
