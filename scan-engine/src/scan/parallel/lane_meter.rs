//! Per-lane wait evidence (F02 FR10.HD1.app-host.DD1).
//!
//! A worker binds the lane of the item it is working on to its thread. Calls
//! into the OS adapter and sends to the observation channel made while bound
//! are counted on that lane, so a sample can name what the lane waits for. A
//! lane without such evidence reports its cause as unconfirmed.
use du_core_contracts::job::LaneWait;
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
        mpsc::{SendError, SyncSender, TrySendError},
    },
};

/// The counts of one lane at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LaneSample {
    pub(super) in_flight: u32,
    pub(super) blocked_sends: u32,
    pub(super) members: usize,
    pub(super) limit: usize,
    pub(super) queued: usize,
}

impl LaneSample {
    /// The lane's wait cause and whether evidence confirms it. The first
    /// match wins: an OS call in flight, a blocked send, queued work at the
    /// worker limit, then an idle lane with no work. Anything else, such as
    /// queued work no worker has taken yet, has no confirmed cause.
    pub(super) fn wait(&self) -> (LaneWait, bool) {
        if self.in_flight > 0 {
            (LaneWait::IoWait, true)
        } else if self.blocked_sends > 0 {
            (LaneWait::Backpressure, true)
        } else if self.queued > 0 && self.members >= self.limit {
            (LaneWait::DeviceCap, true)
        } else {
            (LaneWait::None, self.queued == 0 && self.members == 0)
        }
    }
}

#[derive(Default)]
struct Meter {
    in_flight: AtomicU32,
    blocked_sends: AtomicU32,
    failures: AtomicU64,
}

/// The wait counters of all lanes of one scan.
pub(super) struct LaneMeters {
    lanes: Box<[Meter]>,
}

impl LaneMeters {
    pub(super) fn new(lanes: usize) -> Arc<Self> {
        Arc::new(Self {
            lanes: (0..lanes).map(|_| Meter::default()).collect(),
        })
    }

    /// The OS calls in flight, blocked sends and failures of `lane`.
    pub(super) fn counts(&self, lane: usize) -> (u32, u32, u64) {
        self.lanes.get(lane).map_or((0, 0, 0), |meter| {
            (
                meter.in_flight.load(Ordering::Acquire),
                meter.blocked_sends.load(Ordering::Acquire),
                meter.failures.load(Ordering::Acquire),
            )
        })
    }
}

thread_local! {
    static BOUND: RefCell<Option<(Arc<LaneMeters>, usize)>> = const { RefCell::new(None) };
}

/// Binds a lane to this thread until dropped.
pub(super) struct Binding(());

impl Drop for Binding {
    fn drop(&mut self) {
        BOUND.with(|bound| bound.borrow_mut().take());
    }
}

/// Counts this thread's OS calls and blocked sends on `lane` until the
/// returned binding is dropped.
pub(super) fn bind(meters: &Arc<LaneMeters>, lane: usize) -> Binding {
    BOUND.with(|bound| *bound.borrow_mut() = Some((Arc::clone(meters), lane)));
    Binding(())
}

/// Raises one counter of the bound lane while `work` runs.
fn counted<R>(counter: fn(&Meter) -> &AtomicU32, work: impl FnOnce() -> R) -> R {
    struct Lower(Option<(Arc<LaneMeters>, usize)>, fn(&Meter) -> &AtomicU32);
    impl Drop for Lower {
        fn drop(&mut self) {
            if let Some(meter) = self
                .0
                .as_ref()
                .and_then(|(meters, lane)| meters.lanes.get(*lane))
            {
                self.1(meter).fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
    let bound = BOUND.with(|bound| bound.borrow().clone());
    if let Some(meter) = bound
        .as_ref()
        .and_then(|(meters, lane)| meters.lanes.get(*lane))
    {
        counter(meter).fetch_add(1, Ordering::AcqRel);
    }
    let _lower = Lower(bound, counter);
    work()
}

/// Runs one call into the OS adapter, counted as in flight on the bound lane.
pub(super) fn os_call<R>(call: impl FnOnce() -> R) -> R {
    counted(|meter| &meter.in_flight, call)
}

/// Counts one recorded failure on the bound lane.
pub(super) fn failed() {
    BOUND.with(|bound| {
        if let Some(meter) = bound
            .borrow()
            .as_ref()
            .and_then(|(meters, lane)| meters.lanes.get(*lane))
        {
            meter.failures.fetch_add(1, Ordering::AcqRel);
        }
    });
}

/// Sends `value`; while the channel is full the bound lane counts a blocked send.
pub(super) fn send<T>(sender: &SyncSender<T>, value: T) -> Result<(), SendError<T>> {
    match sender.try_send(value) {
        Ok(()) => Ok(()),
        Err(TrySendError::Disconnected(value)) => Err(SendError(value)),
        Err(TrySendError::Full(value)) => {
            counted(|meter| &meter.blocked_sends, || sender.send(value))
        }
    }
}

#[cfg(test)]
mod tests;
