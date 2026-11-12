//! Deliver WebView2 messages on the STA while application handlers use bounded lanes.
use super::super::contract::{
    Dispatch, DispatchLane, Events, ResponseDeliveryObserver, WebApp, message, trusted_message,
};
use super::{
    folder_drop::{self, Reason},
    folder_validation,
    reply::Reply,
};
use std::{
    cell::Cell,
    io,
    rc::Rc,
    sync::{Arc, mpsc},
};
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

#[derive(Clone)]
struct IpcQueues {
    ordered: mpsc::SyncSender<Request>,
    isolated: mpsc::SyncSender<Request>,
    query_control: mpsc::SyncSender<Request>,
    job_control: mpsc::SyncSender<Request>,
    route: fn(&str) -> DispatchLane,
}

impl IpcQueues {
    fn start(app: &WebApp, replies: mpsc::SyncSender<Reply>) -> io::Result<Self> {
        Ok(Self {
            ordered: worker(
                "diskunfold-ipc",
                app.dispatch.clone(),
                app.response_delivery.clone(),
                replies.clone(),
            )?,
            isolated: worker(
                "diskunfold-ipc-isolated",
                app.dispatch.clone(),
                app.response_delivery.clone(),
                replies.clone(),
            )?,
            query_control: worker(
                "diskunfold-ipc-query-control",
                app.dispatch.clone(),
                app.response_delivery.clone(),
                replies.clone(),
            )?,
            job_control: worker(
                "diskunfold-ipc-job-control",
                app.dispatch.clone(),
                app.response_delivery.clone(),
                replies,
            )?,
            route: app.route,
        })
    }

    fn enqueue(&self, origin: &str, id: &str, data: &str) -> bool {
        if !trusted_message(origin, data) {
            return false;
        }
        let lane = match (self.route)(data) {
            DispatchLane::Ordered => &self.ordered,
            DispatchLane::Volume => &self.isolated,
            DispatchLane::QueryControl => &self.query_control,
            DispatchLane::JobControl => &self.job_control,
        };
        lane.try_send(Request {
            id: id.to_owned(),
            origin: origin.to_owned(),
            data: data.to_owned(),
        })
        .is_ok()
    }
}

fn worker(
    name: &str,
    dispatch: Dispatch,
    observer: Option<ResponseDeliveryObserver>,
    outgoing: mpsc::SyncSender<Reply>,
) -> io::Result<mpsc::SyncSender<Request>> {
    let (requests, incoming) = mpsc::sync_channel::<Request>(QUEUE_LIMIT);
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            while let Ok(request) = incoming.recv() {
                let payload = message(&dispatch, true, &request.origin, &request.data);
                let callback = observer.as_ref().and_then(|observe| {
                    payload
                        .as_deref()
                        .and_then(|response| observe(&request.data, response))
                });
                let reply =
                    Reply::ipc(request.id, payload.as_deref().unwrap_or_default(), callback);
                if outgoing.send(reply).is_err() {
                    break;
                }
            }
        })?;
    Ok(requests)
}

/// A bounded queue for requests accepted from the trusted top-level document.
#[derive(Clone)]
pub(super) struct RequestQueue {
    ipc: IpcQueues,
    folders: mpsc::SyncSender<(u64, Vec<u16>)>,
}

impl RequestQueue {
    pub(super) fn submit(
        &self,
        view: &ICoreWebView2,
        origin: &str,
        id: &str,
        data: &str,
    ) -> windows::core::Result<()> {
        if self.ipc.enqueue(origin, id, data) {
            return Ok(());
        }
        post(view, &format!("{id}\n"))
    }

    /// Schedules local directory verification off the WebView thread.
    pub(super) fn submit_folder(
        &self,
        view: &ICoreWebView2,
        id: u64,
        path: Vec<u16>,
    ) -> windows::core::Result<()> {
        if self.folders.try_send((id, path)).is_ok() {
            return Ok(());
        }
        post(view, &folder_drop::reason_message(id, Reason::Unsupported))
    }
}

pub(super) struct Bridge {
    view: ICoreWebView2,
    ready: Rc<Cell<bool>>,
    requests: RequestQueue,
    replies: mpsc::Receiver<Reply>,
    events: Option<Events>,
}

impl Bridge {
    pub(super) fn start(
        view: ICoreWebView2,
        app: &WebApp,
        events: Option<Events>,
        ready: Rc<Cell<bool>>,
    ) -> io::Result<Self> {
        let (outgoing, replies) = mpsc::sync_channel(QUEUE_LIMIT);
        let requests = IpcQueues::start(app, outgoing.clone())?;
        let folders = folder_validation::start(outgoing, Arc::new(folder_drop::response))?;
        Ok(Self {
            view,
            ready,
            requests: RequestQueue {
                ipc: requests,
                folders,
            },
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
            let _ = reply.deliver_with(|wire| post(&self.view, wire));
        }
        if let Some(events) = &self.events {
            flush_event_batch(events.as_ref(), |wire| post(&self.view, wire), |_| {});
        }
    }
}

fn flush_event_batch<E>(
    mut next: impl FnMut() -> Option<String>,
    mut post: impl FnMut(&str) -> Result<(), E>,
    mut on_post_failure: impl FnMut(usize),
) {
    for (index, payload) in std::iter::from_fn(&mut next).take(QUEUE_LIMIT).enumerate() {
        if post(&format!("event\n{payload}")).is_err() {
            on_post_failure(index);
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

#[cfg(test)]
mod tests;
