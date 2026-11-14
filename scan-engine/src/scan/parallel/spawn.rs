//! Starting the workers of one collection and releasing its queue.
use super::*;

/// Releases the initial directory only after all workers were successfully started.
pub(super) fn start_queue(shared: &Shared) -> Result<(), BuildError> {
    let mut queue = shared.queue.lock().map_err(|_| BuildError::Unavailable)?;
    queue.started = true;
    shared.wake.notify_all();
    Ok(())
}

pub(super) fn spawn_workers<'scope, 'env>(
    scope: &'scope thread::Scope<'scope, 'env>,
    flows: Vec<Box<dyn JobFlow>>,
    shared: &'scope Shared,
    sender: &SyncSender<Event>,
    context: &'scope dyn JobContext,
    policy: Policy<'scope>,
) -> Result<(), BuildError> {
    let mut flows = flows.into_iter();
    #[cfg(test)]
    let mut index = 0;
    while let Some(flow) = flows.next() {
        let sender = sender.clone();
        let slot = Arc::new(Mutex::new(Some(flow)));
        let worker_slot = Arc::clone(&slot);
        #[cfg(test)]
        let fail = FAIL_SPAWN_AT.load(Ordering::Acquire) == index;
        #[cfg(not(test))]
        let fail = false;
        #[cfg(test)]
        if fail && index > 0 {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !TEST_WORKER_STARTED.load(Ordering::Acquire)
                && std::time::Instant::now() < deadline
            {
                thread::yield_now();
            }
        }
        let spawned = if fail {
            Err(std::io::Error::other("injected spawn failure"))
        } else {
            thread::Builder::new().spawn_scoped(scope, move || {
                #[cfg(test)]
                TEST_WORKER_STARTED.store(true, Ordering::Release);
                let flow = worker_slot.lock().ok().and_then(|mut slot| slot.take());
                if let Some(flow) = flow {
                    worker(shared, sender, context, policy, flow);
                } else {
                    shared.abort(context);
                }
            })
        };
        #[cfg(test)]
        {
            index += 1;
        }
        if spawned.is_err() {
            if let Ok(mut slot) = slot.lock()
                && let Some(flow) = slot.take()
            {
                flow.finish(JobCounts::default());
            }
            for flow in flows {
                flow.finish(JobCounts::default());
            }
            shared.abort(context);
            return Err(BuildError::Unavailable);
        }
    }
    Ok(())
}
