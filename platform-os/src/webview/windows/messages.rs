//! Deliver WebView2 messages on the STA while application handlers use bounded lanes.
use super::super::contract::{Dispatch, DispatchLane, Events, WebApp, message, trusted_message};
use super::folder_drop::{self, Reason};
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

#[derive(Clone)]
struct IpcQueues {
    ordered: mpsc::SyncSender<Request>,
    isolated: mpsc::SyncSender<Request>,
    control: mpsc::SyncSender<Request>,
    route: fn(&str) -> DispatchLane,
}

impl IpcQueues {
    fn start(app: &WebApp, replies: mpsc::SyncSender<String>) -> io::Result<Self> {
        Ok(Self {
            ordered: worker("diskunfold-ipc", app.dispatch.clone(), replies.clone())?,
            isolated: worker(
                "diskunfold-ipc-isolated",
                app.dispatch.clone(),
                replies.clone(),
            )?,
            control: worker(
                "diskunfold-ipc-query-control",
                app.dispatch.clone(),
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
            DispatchLane::QueryControl => &self.control,
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
    outgoing: mpsc::SyncSender<String>,
) -> io::Result<mpsc::SyncSender<Request>> {
    let (requests, incoming) = mpsc::sync_channel::<Request>(QUEUE_LIMIT);
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            while let Ok(request) = incoming.recv() {
                let payload =
                    message(&dispatch, true, &request.origin, &request.data).unwrap_or_default();
                let reply = format!("{}\n{payload}", request.id);
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
    replies: mpsc::Receiver<String>,
    events: Option<Events>,
}

impl Bridge {
    pub(super) fn start(
        view: ICoreWebView2,
        app: &WebApp,
        events: Option<Events>,
        ready: Rc<Cell<bool>>,
    ) -> io::Result<Self> {
        let (folders, incoming_folders) = mpsc::sync_channel::<(u64, Vec<u16>)>(4);
        let (outgoing, replies) = mpsc::sync_channel(QUEUE_LIMIT);
        let requests = IpcQueues::start(app, outgoing.clone())?;
        let folder_replies = outgoing.clone();
        std::thread::Builder::new()
            .name("diskunfold-folder-drop".into())
            .spawn(move || {
                while let Ok((id, path)) = incoming_folders.recv() {
                    if folder_replies
                        .send(folder_drop::response(id, &path))
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
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
            let _ = post(&self.view, &reply);
        }
        if let Some(events) = &self.events {
            for payload in std::iter::from_fn(events.as_ref()).take(QUEUE_LIMIT) {
                let _ = post(&self.view, &format!("event\n{payload}"));
            }
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
mod tests {
    use super::*;
    use crate::webview::contract::entry;
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[test]
    fn a_blocked_list_lane_does_not_queue_scope_review_behind_it()
    -> Result<(), Box<dyn std::error::Error>> {
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = Mutex::new(gate);
        let deadline = Duration::from_secs(5);
        let app = WebApp {
            asset: |_| None,
            dispatch: Arc::new(move |command| {
                if command == "list" {
                    entered.send(()).ok()?;
                    gate.lock().ok()?.recv_timeout(deadline).ok()?;
                }
                Some(command.to_owned())
            }),
            route: |command| {
                if command == "list" {
                    DispatchLane::Volume
                } else {
                    DispatchLane::Ordered
                }
            },
            icon: &[],
        };
        let (send, replies) = mpsc::sync_channel(2);
        let queues = IpcQueues::start(&app, send)?;
        assert!(!queues.enqueue("https://example.invalid", "bad", "list"));
        assert!(queues.enqueue(&entry(), "1", "list"));
        started.recv_timeout(deadline)?;
        assert!(queues.enqueue(&entry(), "2", "review"));
        assert_eq!(replies.recv_timeout(deadline)?, "2\nreview");
        release.send(())?;
        assert_eq!(replies.recv_timeout(deadline)?, "1\nlist");
        Ok(())
    }
    #[test]
    fn original_ordered_lane_blocks_close_behind_open() -> Result<(), Box<dyn std::error::Error>> {
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = Mutex::new(gate);
        let deadline = Duration::from_secs(5);
        let app = WebApp {
            asset: |_| None,
            dispatch: Arc::new(move |command| {
                if command == "browse.open" {
                    entered.send(()).ok()?;
                    gate.lock().ok()?.recv_timeout(deadline).ok()?;
                }
                Some(command.to_owned())
            }),
            // This reproduces the original app-host classification.
            route: |command| {
                if command == "target.list" {
                    DispatchLane::Volume
                } else {
                    DispatchLane::Ordered
                }
            },
            icon: &[],
        };
        let (send, replies) = mpsc::sync_channel(4);
        let queues = IpcQueues::start(&app, send)?;
        assert!(queues.enqueue(&entry(), "1", "browse.open"));
        started.recv_timeout(deadline)?;
        assert!(queues.enqueue(&entry(), "2", "query.region.close"));
        assert!(matches!(
            replies.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release.send(())?;
        assert_eq!(replies.recv_timeout(deadline)?, "1\nbrowse.open");
        assert_eq!(replies.recv_timeout(deadline)?, "2\nquery.region.close");
        Ok(())
    }

    #[test]
    fn query_controls_overtake_blocked_open_and_volume_without_reordering_normal_requests()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (entered, started) = mpsc::channel();
        let (release_open, open_gate) = mpsc::channel();
        let (release_volume, volume_gate) = mpsc::channel();
        let open_gate = Mutex::new(open_gate);
        let volume_gate = Mutex::new(volume_gate);
        let closed = Arc::new(AtomicBool::new(false));
        let state = closed.clone();
        let deadline = Duration::from_secs(5);
        let app = WebApp {
            asset: |_| None,
            dispatch: Arc::new(move |command| match command {
                "browse.open" => {
                    entered.send("open").ok()?;
                    open_gate.lock().ok()?.recv_timeout(deadline).ok()?;
                    Some(
                        if state.load(Ordering::Acquire) {
                            "ref.unavailable"
                        } else {
                            "published"
                        }
                        .to_owned(),
                    )
                }
                "target.list" => {
                    entered.send("volume").ok()?;
                    volume_gate.lock().ok()?.recv_timeout(deadline).ok()?;
                    Some("volume".into())
                }
                "query.cancel" => Some("ref.unavailable".into()),
                "query.region.close" => {
                    state.store(true, Ordering::Release);
                    Some("closed".into())
                }
                other => Some(other.to_owned()),
            }),
            route: |command| match command {
                "target.list" => DispatchLane::Volume,
                "query.cancel" | "query.region.close" => DispatchLane::QueryControl,
                _ => DispatchLane::Ordered,
            },
            icon: &[],
        };
        let (send, replies) = mpsc::sync_channel(16);
        let queues = IpcQueues::start(&app, send)?;
        assert!(queues.enqueue(&entry(), "1", "browse.open"));
        assert!(queues.enqueue(&entry(), "2", "target.list"));
        let mut both = vec![
            started.recv_timeout(deadline)?,
            started.recv_timeout(deadline)?,
        ];
        both.sort();
        assert_eq!(both, ["open", "volume"]);
        assert!(queues.enqueue(&entry(), "3", "normal.first"));
        assert!(queues.enqueue(&entry(), "4", "normal.second"));
        assert!(!queues.enqueue("https://example.invalid", "untrusted", "query.region.close"));
        assert!(queues.enqueue(&entry(), "5", "query.cancel"));
        assert_eq!(replies.recv_timeout(deadline)?, "5\nref.unavailable");
        assert!(!closed.load(Ordering::Acquire));
        assert!(queues.enqueue(&entry(), "6", "query.region.close"));
        assert_eq!(replies.recv_timeout(deadline)?, "6\nclosed");
        release_open.send(())?;
        assert_eq!(replies.recv_timeout(deadline)?, "1\nref.unavailable");
        assert_eq!(replies.recv_timeout(deadline)?, "3\nnormal.first");
        assert_eq!(replies.recv_timeout(deadline)?, "4\nnormal.second");
        release_volume.send(())?;
        assert_eq!(replies.recv_timeout(deadline)?, "2\nvolume");
        Ok(())
    }
}
