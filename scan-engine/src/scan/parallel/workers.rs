//! Worker scheduling and bounded per-root directory processing.
use super::*;

pub(super) fn worker(
    shared: &Shared,
    sender: SyncSender<Event>,
    context: &dyn JobContext,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
    flow: Box<dyn JobFlow>,
) {
    let mut local = Vec::new();
    let mut prefer_shared = true;
    while context.checkpoint() != Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        let frame = if prefer_shared {
            take_ready(shared).or_else(|| local.pop())
        } else {
            local.pop().or_else(|| take_ready(shared))
        }
        .or_else(|| take(shared, context));
        prefer_shared = !prefer_shared;
        let Some(frame) = frame else {
            break;
        };
        if !process(frame, shared, &sender, context, include, &mut local) {
            shared.abort.store(true, Ordering::Release);
            shared.wake.notify_all();
            break;
        }
    }
    flow.finish(JobCounts::default());
}

fn take_ready(shared: &Shared) -> Option<Frame> {
    let mut queue = shared.queue.lock().ok()?;
    if queue.started {
        queue.pop_fair()
    } else {
        None
    }
}

fn take(shared: &Shared, context: &dyn JobContext) -> Option<Frame> {
    let mut queue = shared.queue.lock().ok()?;
    loop {
        if queue.remaining == 0
            || context.checkpoint() == Checkpoint::Cancel
            || shared.abort.load(Ordering::Acquire)
        {
            return None;
        }
        if queue.started
            && let Some(frame) = queue.pop_fair()
        {
            return Some(frame);
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
    if let Ok(mut queue) = shared.queue.lock() {
        if queue.ready.len() < QUEUED_DIRECTORIES {
            queue.ready.push_back(frame);
            shared.wake.notify_one();
            return;
        }
    } else {
        shared.abort.store(true, Ordering::Release);
        shared.wake.notify_all();
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
    if let Ok(mut queue) = shared.queue.lock() {
        queue.remaining += 1;
        queue.root_remaining[frame.root_index as usize] += 1;
        if queue.ready.len() < QUEUED_DIRECTORIES {
            queue.ready.push_back(frame);
            shared.wake.notify_one();
            return None;
        }
    } else {
        shared.abort.store(true, Ordering::Release);
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
