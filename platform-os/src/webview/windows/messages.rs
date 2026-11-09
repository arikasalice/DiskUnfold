//! Deliver WebView2 messages on the STA while application handlers run on a worker.
use super::super::contract::{Dispatch, Events, message, trusted_message};
use std::{cell::Cell, io, rc::Rc, sync::mpsc};
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

struct Request {
    id: String,
    origin: String,
    data: String,
}

/// A bounded queue for requests accepted from the trusted top-level document.
#[derive(Clone)]
pub(super) struct RequestQueue(mpsc::SyncSender<Request>);

impl RequestQueue {
    pub(super) fn submit(
        &self,
        view: &ICoreWebView2,
        origin: &str,
        id: &str,
        data: &str,
    ) -> windows::core::Result<()> {
        if trusted_message(origin, data)
            && self
                .0
                .try_send(Request {
                    id: id.to_owned(),
                    origin: origin.to_owned(),
                    data: data.to_owned(),
                })
                .is_ok()
        {
            return Ok(());
        }
        post(view, &format!("{id}\n"))
    }
}

pub(super) struct Bridge {
    view: ICoreWebView2,
    ready: Rc<Cell<bool>>,
    requests: RequestQueue,
    replies: mpsc::Receiver<String>,
    events: Option<Events>,
}

impl Bridge {
    pub(super) fn start(
        view: ICoreWebView2,
        dispatch: Dispatch,
        events: Option<Events>,
        ready: Rc<Cell<bool>>,
    ) -> io::Result<Self> {
        let (requests, incoming) = mpsc::sync_channel::<Request>(QUEUE_LIMIT);
        let (outgoing, replies) = mpsc::sync_channel(QUEUE_LIMIT);
        std::thread::Builder::new()
            .name("diskunfold-ipc".into())
            .spawn(move || {
                while let Ok(request) = incoming.recv() {
                    let payload = message(&dispatch, true, &request.origin, &request.data)
                        .unwrap_or_default();
                    if outgoing.send(format!("{}\n{payload}", request.id)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            view,
            ready,
            requests: RequestQueue(requests),
            replies,
            events,
        })
    }

    pub(super) fn requests(&self) -> RequestQueue {
        self.requests.clone()
    }

    pub(super) fn flush(&self) {
        if !self.ready.get() {
            return;
        }
        for reply in self.replies.try_iter().take(QUEUE_LIMIT) {
            let _ = post(&self.view, &reply);
        }
        if let Some(events) = &self.events {
            for payload in std::iter::from_fn(events.as_ref()).take(QUEUE_LIMIT) {
                let _ = post(&self.view, &format!("event\n{payload}"));
            }
        }
    }
}

fn post(view: &ICoreWebView2, data: &str) -> windows::core::Result<()> {
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
