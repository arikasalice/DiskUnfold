//! Worker scheduling and bounded per-root directory processing.
use super::*;

pub(super) fn worker(
    shared: &Shared,
    sender: SyncSender<Event>,
    context: &dyn JobContext,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
    flow: Box<dyn JobFlow>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        worker_loop(shared, &sender, context, include)
    }));
    if !matches!(result, Ok(true)) {
        shared.abort();
    }
    flow.finish(JobCounts::default());
}

fn worker_loop(
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
) -> bool {
    let mut local = Vec::new();
    let mut prefer_shared = true;
    while context.checkpoint() != Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        let claim = take(shared, context, &mut local, prefer_shared);
        prefer_shared = !prefer_shared;
        let Some(Claim {
            frame,
            permit: _permit,
        }) = claim
        else {
            break;
        };
        if !process(frame, shared, sender, context, include, &mut local) {
            return false;
        }
    }
    true
}

struct Permit<'a> {
    shared: &'a Shared,
    group: usize,
}
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.active_groups[self.group] -= 1;
        self.shared.wake.notify_all();
    }
}
struct Claim<'a> {
    frame: Frame,
    permit: Permit<'a>,
}

fn claim_ready<'a>(shared: &'a Shared, queue: &mut Queue) -> Option<Claim<'a>> {
    if queue.started {
        let (frame, group) = queue.pop_fair()?;
        Some(Claim {
            frame,
            permit: Permit { shared, group },
        })
    } else {
        None
    }
}

fn claim_local<'a>(
    shared: &'a Shared,
    queue: &mut Queue,
    local: &mut Vec<Frame>,
) -> Option<Claim<'a>> {
    if !queue.started {
        return None;
    }
    let index = local.iter().rposition(|frame| {
        let group = queue.root_groups[frame.root_index as usize];
        queue.active_groups[group] < queue.device_limit
    })?;
    let frame = local.remove(index);
    let group = queue.root_groups[frame.root_index as usize];
    queue.active_groups[group] += 1;
    Some(Claim {
        frame,
        permit: Permit { shared, group },
    })
}

fn take<'a>(
    shared: &'a Shared,
    context: &dyn JobContext,
    local: &mut Vec<Frame>,
    prefer_shared: bool,
) -> Option<Claim<'a>> {
    let mut queue = shared.queue.lock().ok()?;
    loop {
        if context.checkpoint() == Checkpoint::Cancel || shared.abort.load(Ordering::Acquire) {
            return None;
        }
        let claim = if prefer_shared {
            claim_ready(shared, &mut queue).or_else(|| claim_local(shared, &mut queue, local))
        } else {
            claim_local(shared, &mut queue, local).or_else(|| claim_ready(shared, &mut queue))
        };
        if claim.is_some() {
            return claim;
        }
        if queue.remaining == 0 {
            return None;
        }
        queue = shared
            .wake
            .wait_timeout(queue, Duration::from_millis(25))
            .ok()?
            .0;
    }
}

fn process(
    mut frame: Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
    local: &mut Vec<Frame>,
) -> bool {
    while context.checkpoint() != Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        let recorded = frame.root_published && !frame.deferred.is_empty();
        let next = if recorded {
            frame.deferred.pop_front()
        } else {
            frame.entries.pop_front()
        };
        if let Some(entry) = next {
            match process_entry(
                &mut frame, entry, recorded, shared, sender, context, include,
            ) {
                Ok(Some(child)) => {
                    local.push(frame);
                    local.push(child);
                    return true;
                }
                Ok(None) => {}
                Err(()) => return false,
            }
            continue;
        }
        if !publish_first_batch(&mut frame, sender) {
            return false;
        }
        if frame.batch_pending {
            frame.batch_pending = false;
            requeue(shared, frame, local);
            return true;
        }
        match frame.directory.next_batch() {
            Ok(batch) if batch.is_empty() => {
                if frame.root && sender.send(Event::RootBatch(frame.root_index)).is_err() {
                    return false;
                }
                complete(shared, frame.root_index);
                return true;
            }
            Ok(batch) if batch.len() <= 8192 => {
                frame.first_batch |= frame.root && !frame.root_published;
                frame.batch_pending = true;
                frame.entries = batch.into();
            }
            outcome => {
                let failure_reason = match outcome {
                    Err(error) => reason(&error),
                    _ => "scan.unclassified",
                };
                let _ = sender.send(Event::Failure {
                    root_index: frame.root_index,
                    path: frame.path.clone(),
                    root: frame.root,
                    reason: failure_reason,
                });
                complete(shared, frame.root_index);
                return true;
            }
        }
    }
    true
}

fn publish_first_batch(frame: &mut Frame, sender: &SyncSender<Event>) -> bool {
    if frame.root && frame.first_batch {
        frame.first_batch = false;
        if sender.send(Event::RootBatch(frame.root_index)).is_err() {
            return false;
        }
        frame.root_published = true;
    }
    true
}

fn process_entry(
    frame: &mut Frame,
    entry: ObservedEntry,
    recorded: bool,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
) -> Result<Option<Frame>, ()> {
    let mut path = frame.path.clone();
    path.push(entry.name.clone());
    if !recorded && !include(&path) {
        return Ok(None);
    }
    if !recorded {
        sender
            .send(Event::Item(
                frame.root_index,
                path.clone(),
                Box::new(entry.clone()),
            ))
            .map_err(|_| ())?;
    }
    if !eligible_directory(&entry) || context.checkpoint() == Checkpoint::Cancel {
        return Ok(None);
    }
    if frame.root && !frame.root_published {
        frame.deferred.push_back(entry);
        return Ok(None);
    }
    match frame.directory.open_child(&entry) {
        Ok(directory) => Ok(schedule(
            shared,
            Frame {
                root_index: frame.root_index,
                path,
                directory,
                entries: VecDeque::new(),
                deferred: VecDeque::new(),
                root: false,
                first_batch: false,
                root_published: false,
                batch_pending: false,
            },
        )),
        Err(error) => {
            sender
                .send(Event::Failure {
                    root_index: frame.root_index,
                    path,
                    root: false,
                    reason: reason(&error),
                })
                .map_err(|_| ())?;
            Ok(None)
        }
    }
}
fn requeue(shared: &Shared, frame: Frame, local: &mut Vec<Frame>) {
    let mut queue = match shared.queue.lock() {
        Ok(queue) => queue,
        Err(error) => {
            let _queue = error.into_inner();
            shared.abort.store(true, Ordering::Release);
            shared.wake.notify_all();
            local.push(frame);
            return;
        }
    };
    if queue.ready.len() < QUEUED_DIRECTORIES {
        queue.ready.push_back(frame);
        shared.wake.notify_one();
        return;
    }
    local.push(frame);
}

fn eligible_directory(entry: &ObservedEntry) -> bool {
    entry.metadata.as_ref().is_ok_and(|m| {
        m.kind == ObservedKind::Directory
            && m.inode != 0
            && m.inode == entry.enumerated_inode
            && (entry.enumerated_kind == ObservedKind::Unknown || entry.enumerated_kind == m.kind)
    })
}
fn schedule(shared: &Shared, frame: Frame) -> Option<Frame> {
    let mut queue = match shared.queue.lock() {
        Ok(queue) => queue,
        Err(error) => {
            let _queue = error.into_inner();
            shared.abort.store(true, Ordering::Release);
            shared.wake.notify_all();
            return Some(frame);
        }
    };
    queue.remaining += 1;
    queue.root_remaining[frame.root_index as usize] += 1;
    if queue.ready.len() < QUEUED_DIRECTORIES {
        queue.ready.push_back(frame);
        shared.wake.notify_one();
        return None;
    }
    Some(frame)
}
fn complete(shared: &Shared, root_index: u32) {
    if let Ok(mut queue) = shared.queue.lock() {
        queue.remaining -= 1;
        queue.root_remaining[root_index as usize] -= 1;
        shared.wake.notify_all();
    }
}

#[cfg(test)]
mod permit_tests {
    use super::*;
    use du_core_contracts::{
        ports::{PortError, PortErrorKind},
        scan_observation::DirectoryObservations,
    };
    struct Empty;
    impl DirectoryObservations for Empty {
        fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
            Ok(Vec::new())
        }
    }
    impl AnchoredDirectory for Empty {
        fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
            Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            })
        }
    }
    #[test]
    fn permit_is_released_during_panic_unwind() {
        let frame = Frame {
            root_index: 0,
            path: Vec::new(),
            directory: Box::new(Empty),
            entries: VecDeque::new(),
            deferred: VecDeque::new(),
            root: true,
            first_batch: false,
            root_published: false,
            batch_pending: false,
        };
        let shared = Shared {
            queue: Mutex::new(Queue {
                ready: VecDeque::from([frame]),
                last_root: None,
                remaining: 1,
                root_remaining: vec![1],
                root_groups: vec![0],
                active_groups: vec![0],
                device_limit: 1,
                started: true,
            }),
            wake: Condvar::new(),
            abort: AtomicBool::new(false),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut queue = shared
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let claim = claim_ready(&shared, &mut queue);
            drop(queue);
            let _claim = claim;
            assert!(
                shared
                    .queue
                    .lock()
                    .is_ok_and(|queue| queue.active_groups[0] == 1)
            );
            std::panic::resume_unwind(Box::new("injected panic"));
        }));
        assert!(result.is_err());
        assert!(
            shared
                .queue
                .lock()
                .is_ok_and(|queue| queue.active_groups[0] == 0)
        );
    }
}
