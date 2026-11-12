use super::super::super::contract::{
    Dispatch, DispatchLane, ResponseDeliveryObserver, WebApp, message,
};
use super::super::{
    folder_drop,
    folder_validation::{self, FolderVerifier},
    reply::Reply,
};
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::{self, JoinHandle},
};

use super::QUEUE_LIMIT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lane {
    Ordered,
    Isolated,
    QueryControl,
    JobControl,
}

impl From<DispatchLane> for Lane {
    fn from(lane: DispatchLane) -> Self {
        match lane {
            DispatchLane::Ordered => Self::Ordered,
            DispatchLane::Volume => Self::Isolated,
            DispatchLane::QueryControl => Self::QueryControl,
            DispatchLane::JobControl => Self::JobControl,
        }
    }
}

pub(super) struct Request {
    pub(super) generation: u64,
    pub(super) id: String,
    pub(super) origin: String,
    pub(super) data: String,
}

pub(super) struct FolderRequest {
    pub(super) generation: u64,
    pub(super) id: u64,
    pub(super) path: Vec<u16>,
}

struct ScopedReply {
    pub(super) generation: u64,
    reply: Reply,
}

#[derive(Default)]
struct Queues {
    active_generation: Option<u64>,
    next_generation: u64,
    shutdown: bool,
    ordered: VecDeque<Request>,
    isolated: VecDeque<Request>,
    query_control: VecDeque<Request>,
    job_control: VecDeque<Request>,
    folders: VecDeque<FolderRequest>,
    replies: VecDeque<ScopedReply>,
}

impl Queues {
    fn lane_mut(&mut self, lane: Lane) -> &mut VecDeque<Request> {
        match lane {
            Lane::Ordered => &mut self.ordered,
            Lane::Isolated => &mut self.isolated,
            Lane::QueryControl => &mut self.query_control,
            Lane::JobControl => &mut self.job_control,
        }
    }

    fn clear_requests(&mut self) {
        self.ordered.clear();
        self.isolated.clear();
        self.query_control.clear();
        self.job_control.clear();
        self.folders.clear();
    }
}

pub(super) struct Shared {
    queues: Mutex<Queues>,
    changed: Condvar,
}

impl Shared {
    fn new() -> Self {
        Self {
            queues: Mutex::new(Queues::default()),
            changed: Condvar::new(),
        }
    }

    fn activate(self: &Arc<Self>) -> io::Result<ControllerOwner> {
        let (generation, stale_replies) = {
            let mut queues = self.lock()?;
            if queues.shutdown {
                return Err(io::Error::other("WebView worker pool is shut down"));
            }
            let generation = queues
                .next_generation
                .checked_add(1)
                .ok_or_else(|| io::Error::other("WebView generation space exhausted"))?;
            queues.next_generation = generation;
            queues.active_generation = Some(generation);
            queues.clear_requests();
            (generation, std::mem::take(&mut queues.replies))
        };
        self.changed.notify_all();
        drop(stale_replies);
        Ok(ControllerOwner {
            shared: Arc::clone(self),
            generation,
        })
    }

    fn lock(&self) -> io::Result<MutexGuard<'_, Queues>> {
        self.queues
            .lock()
            .map_err(|_| io::Error::other("WebView worker state is poisoned"))
    }

    pub(super) fn is_active(&self, generation: u64) -> bool {
        self.lock()
            .is_ok_and(|queues| !queues.shutdown && queues.active_generation == Some(generation))
    }

    pub(super) fn enqueue_ipc(&self, request: Request, lane: Lane) -> EnqueueResult {
        let Ok(mut queues) = self.lock() else {
            return EnqueueResult::Closed;
        };
        if queues.shutdown || queues.active_generation != Some(request.generation) {
            return EnqueueResult::Stale;
        }
        let queue = queues.lane_mut(lane);
        if queue.len() >= QUEUE_LIMIT {
            return EnqueueResult::Full;
        }
        queue.push_back(request);
        self.changed.notify_all();
        EnqueueResult::Accepted
    }

    pub(super) fn enqueue_folder(&self, request: FolderRequest) -> EnqueueResult {
        let Ok(mut queues) = self.lock() else {
            return EnqueueResult::Closed;
        };
        if queues.shutdown || queues.active_generation != Some(request.generation) {
            return EnqueueResult::Stale;
        }
        if queues.folders.len() >= folder_validation::WAITING_LIMIT {
            return EnqueueResult::Full;
        }
        queues.folders.push_back(request);
        self.changed.notify_all();
        EnqueueResult::Accepted
    }

    fn take_ipc(&self, lane: Lane) -> Option<Request> {
        let mut queues = self.lock().ok()?;
        loop {
            if queues.shutdown {
                return None;
            }
            if let Some(request) = queues.lane_mut(lane).pop_front() {
                if queues.active_generation == Some(request.generation) {
                    // Popping under the same lock as revocation is the dispatch-start
                    // linearization point. A request claimed first may finish; a request
                    // revoked first is removed without invoking the application handler.
                    return Some(request);
                }
                continue;
            }
            queues = self.changed.wait(queues).ok()?;
        }
    }

    fn take_folder(&self) -> Option<FolderRequest> {
        let mut queues = self.lock().ok()?;
        loop {
            if queues.shutdown {
                return None;
            }
            if let Some(request) = queues.folders.pop_front() {
                if queues.active_generation == Some(request.generation) {
                    // This claim is atomic with generation revocation, as for IPC work.
                    return Some(request);
                }
                continue;
            }
            queues = self.changed.wait(queues).ok()?;
        }
    }

    fn publish_reply(&self, generation: u64, reply: Reply) {
        let Ok(mut queues) = self.lock() else {
            return;
        };
        loop {
            if queues.shutdown || queues.active_generation != Some(generation) {
                drop(queues);
                drop(reply);
                return;
            }
            if queues.replies.len() < QUEUE_LIMIT {
                queues.replies.push_back(ScopedReply { generation, reply });
                self.changed.notify_all();
                return;
            }
            let Ok(next) = self.changed.wait(queues) else {
                return;
            };
            queues = next;
        }
    }

    fn take_replies(&self, generation: u64) -> Vec<Reply> {
        let Ok(mut queues) = self.lock() else {
            return Vec::new();
        };
        if queues.shutdown || queues.active_generation != Some(generation) {
            return Vec::new();
        }
        let pending = std::mem::take(&mut queues.replies);
        let mut current = Vec::with_capacity(pending.len());
        let mut stale = VecDeque::new();
        for scoped in pending {
            if scoped.generation == generation {
                current.push(scoped.reply);
            } else {
                stale.push_back(scoped);
            }
        }
        self.changed.notify_all();
        drop(queues);
        drop(stale);
        current
    }

    #[cfg(test)]
    fn take_reply(&self, generation: u64) -> Option<Reply> {
        let mut queues = self.lock().ok()?;
        if queues.shutdown || queues.active_generation != Some(generation) {
            return None;
        }
        let scoped = queues.replies.pop_front()?;
        self.changed.notify_all();
        drop(queues);
        if scoped.generation == generation {
            Some(scoped.reply)
        } else {
            drop(scoped);
            None
        }
    }

    #[cfg(test)]
    fn reply_count(&self, generation: u64) -> usize {
        self.lock()
            .ok()
            .filter(|queues| queues.active_generation == Some(generation))
            .map_or(0, |queues| queues.replies.len())
    }

    fn revoke(&self, generation: u64) {
        let Ok(mut queues) = self.lock() else {
            self.changed.notify_all();
            return;
        };
        if queues.active_generation != Some(generation) {
            return;
        }
        queues.active_generation = None;
        queues.clear_requests();
        let stale_replies = std::mem::take(&mut queues.replies);
        self.changed.notify_all();
        drop(queues);
        drop(stale_replies);
    }

    fn shutdown(&self) {
        let Ok(mut queues) = self.lock() else {
            self.changed.notify_all();
            return;
        };
        queues.shutdown = true;
        queues.active_generation = None;
        queues.clear_requests();
        let stale_replies = std::mem::take(&mut queues.replies);
        self.changed.notify_all();
        drop(queues);
        drop(stale_replies);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EnqueueResult {
    Accepted,
    Full,
    Stale,
    Closed,
}

/// Owns the fixed worker set for one native HWND.
///
/// The pool creates four IPC workers and four folder validators once. Controller
/// owners reuse those workers and replace only their generation-scoped request and
/// reply state. Dropping the pool wakes workers and detaches their join handles so
/// a handler already blocked in application work cannot hang window shutdown.
pub(in crate::webview::windows) struct WorkerPool {
    shared: Arc<Shared>,
    _workers: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    /// Starts the HWND's bounded worker set with the production folder verifier.
    pub(in crate::webview::windows) fn start(app: &WebApp) -> io::Result<Self> {
        Self::start_with_folder_verifier(app, Arc::new(folder_drop::response))
    }

    pub(super) fn start_with_folder_verifier(
        app: &WebApp,
        verifier: Arc<FolderVerifier>,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared::new());
        let mut workers = Vec::with_capacity(4 + folder_validation::WORKER_COUNT);
        let mut spawn = |result: io::Result<JoinHandle<()>>| -> io::Result<()> {
            match result {
                Ok(worker) => {
                    workers.push(worker);
                    Ok(())
                }
                Err(error) => Err(error),
            }
        };
        let setup = (|| {
            for (name, lane) in [
                ("diskunfold-ipc", Lane::Ordered),
                ("diskunfold-ipc-isolated", Lane::Isolated),
                ("diskunfold-ipc-query-control", Lane::QueryControl),
                ("diskunfold-ipc-job-control", Lane::JobControl),
            ] {
                spawn(spawn_ipc_worker(
                    name,
                    Arc::clone(&shared),
                    lane,
                    app.dispatch.clone(),
                    app.response_delivery.clone(),
                ))?;
            }
            for index in 0..folder_validation::WORKER_COUNT {
                spawn(spawn_folder_worker(
                    index,
                    Arc::clone(&shared),
                    Arc::clone(&verifier),
                ))?;
            }
            Ok(())
        })();
        drop(spawn);
        if let Err(error) = setup {
            shared.shutdown();
            for worker in workers {
                let _ = worker.join();
            }
            return Err(error);
        }
        Ok(Self {
            shared,
            _workers: workers,
        })
    }

    /// Creates a controller-scoped owner without starting additional threads.
    pub(in crate::webview::windows) fn controller_owner(&self) -> io::Result<ControllerOwner> {
        self.shared.activate()
    }

    #[cfg(test)]
    pub(super) fn worker_count(&self) -> usize {
        self._workers.len()
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.shared.shutdown();
        // Dropping JoinHandle detaches the worker. Dispatch and verification callbacks
        // are synchronous and cannot be forcibly interrupted when a window is closing.
    }
}

fn spawn_ipc_worker(
    name: &str,
    shared: Arc<Shared>,
    lane: Lane,
    dispatch: Dispatch,
    observer: Option<ResponseDeliveryObserver>,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new().name(name.into()).spawn(move || {
        while let Some(request) = shared.take_ipc(lane) {
            let payload = message(&dispatch, true, &request.origin, &request.data);
            let callback = observer.as_ref().and_then(|observe| {
                payload
                    .as_deref()
                    .and_then(|response| observe(&request.data, response))
            });
            let reply = Reply::ipc(request.id, payload.as_deref().unwrap_or_default(), callback);
            shared.publish_reply(request.generation, reply);
        }
    })
}

fn spawn_folder_worker(
    index: usize,
    shared: Arc<Shared>,
    verifier: Arc<FolderVerifier>,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name(format!("diskunfold-folder-drop-{index}"))
        .spawn(move || {
            while let Some(request) = shared.take_folder() {
                let reply = Reply::plain(verifier(request.id, &request.path));
                shared.publish_reply(request.generation, reply);
            }
        })
}

/// Owns one controller generation while reusing the HWND's fixed worker set.
///
/// Dropping the owner revokes its queued requests and replies. A request claimed by
/// a worker before revocation may finish, but its reply is discarded unless its
/// generation is still active.
pub(in crate::webview::windows) struct ControllerOwner {
    shared: Arc<Shared>,
    pub(super) generation: u64,
}

impl ControllerOwner {
    pub(super) fn requests(&self, route: fn(&str) -> DispatchLane) -> super::RequestQueue {
        super::RequestQueue::new(Arc::clone(&self.shared), self.generation, route)
    }

    pub(super) fn take_replies(&self) -> Vec<Reply> {
        self.shared.take_replies(self.generation)
    }

    #[cfg(test)]
    pub(super) fn take_reply(&self) -> Option<Reply> {
        self.shared.take_reply(self.generation)
    }

    #[cfg(test)]
    pub(super) fn reply_count(&self) -> usize {
        self.shared.reply_count(self.generation)
    }

    pub(super) fn is_active(&self) -> bool {
        self.shared.is_active(self.generation)
    }
}

impl ControllerOwner {
    pub(super) fn revoke(&self) {
        self.shared.revoke(self.generation);
    }
}

impl Drop for ControllerOwner {
    fn drop(&mut self) {
        self.revoke();
    }
}
