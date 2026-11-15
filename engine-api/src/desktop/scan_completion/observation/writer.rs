//! Stores prepared observation rows on a worker thread of their own.
//!
//! The job event delivery thread only hands a row over with [`ObservationWriter::submit`], which
//! never touches the database and never waits. One worker takes the rows one at a time, so a
//! locked database delays later observations but not the delivery of job events.
//!
//! The queue is bounded. A full queue drops the new row and says so in a diagnostic line,
//! because an observation is a convenience that must never hold back or change a scan.

use du_core_contracts::scope_observation_log::{
    ScopeObservation, ScopeObservationLog, ScopeObservationWriteError,
};
use std::{
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, TrySendError},
    },
    thread::{self, JoinHandle, Thread},
    time::{Duration, Instant},
};

use super::Failure;

/// Rows that may wait for the worker. Scans end rarely, so a full queue means the database
/// has been unavailable for a long time; keeping more rows would only keep stale work.
pub(crate) const QUEUE_CAPACITY: usize = 64;

/// Pauses before the second and third attempt at a row whose write found the database busy.
///
/// The storage port makes one attempt per call and waits at most the connection's busy timeout
/// (five seconds). A busy answer therefore means another writer held the lock for that whole
/// time, so retrying at once would only wait the same time again. The pauses give the holder
/// time to finish and let other users of the shared connection take their turn. Three attempts
/// keep the worst case for one row near twenty seconds; only this worker waits for it.
const BUSY_PAUSES: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];

/// How long after a close request the worker keeps writing queued rows. Rows still queued
/// after it are dropped. A normal quit holds a handful of rows that take milliseconds.
const DRAIN_AFTER_CLOSE: Duration = Duration::from_secs(1);

/// How long the first close request waits on the caller's thread for the worker to finish.
/// The shutdown sequence polls again on its next ticks, so the wait stays short.
const FIRST_CLOSE_WAIT: Duration = Duration::from_millis(500);

/// Timing of one writer; tests shorten it.
pub(crate) struct Policy {
    pub(crate) busy_pauses: Vec<Duration>,
    pub(crate) drain_after_close: Duration,
    pub(crate) first_close_wait: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            busy_pauses: BUSY_PAUSES.to_vec(),
            drain_after_close: DRAIN_AFTER_CLOSE,
            first_close_wait: FIRST_CLOSE_WAIT,
        }
    }
}

/// Why a row was not queued. Neither case changes the scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropReason {
    /// The worker has `QUEUE_CAPACITY` rows waiting.
    QueueFull,
    /// The writer is closing or its worker has ended.
    Closed,
}

impl DropReason {
    /// A value-free key for the diagnostic line.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::QueueFull => "queue_full",
            Self::Closed => "writer_closed",
        }
    }
}

enum Item {
    Row(Box<ScopeObservation>),
    #[cfg(test)]
    Fence(mpsc::SyncSender<()>),
}

#[derive(Default)]
struct State {
    closing: AtomicBool,
    discard_after: OnceLock<Instant>,
}

impl State {
    fn closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }

    fn expired(&self) -> bool {
        self.discard_after
            .get()
            .is_some_and(|limit| Instant::now() >= *limit)
    }

    /// Sleeps for `length`, or until a close request ends the wait.
    fn pause(&self, length: Duration) {
        let end = Instant::now() + length;
        while !self.closing() {
            let now = Instant::now();
            if now >= end {
                break;
            }
            thread::park_timeout(end - now);
        }
    }
}

/// The queue and the one worker that writes observation rows through the injected log.
pub(crate) struct ObservationWriter {
    sender: Mutex<Option<mpsc::SyncSender<Item>>>,
    state: Arc<State>,
    worker: Mutex<Option<JoinHandle<()>>>,
    waker: Thread,
    policy: Policy,
}

impl ObservationWriter {
    /// Starts the worker with the production timing.
    pub(crate) fn start(log: Arc<dyn ScopeObservationLog>) -> io::Result<Self> {
        Self::start_with(log, Policy::default())
    }

    pub(crate) fn start_with(
        log: Arc<dyn ScopeObservationLog>,
        policy: Policy,
    ) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let state = Arc::new(State::default());
        let worker_state = state.clone();
        let pauses = policy.busy_pauses.clone();
        let worker = thread::Builder::new()
            .name("diskunfold-scope-observation".into())
            .spawn(move || run(&receiver, log.as_ref(), &worker_state, &pauses))?;
        Ok(Self {
            sender: Mutex::new(Some(sender)),
            state,
            waker: worker.thread().clone(),
            worker: Mutex::new(Some(worker)),
            policy,
        })
    }

    /// Queues one row without waiting. A dropped row is reported in a diagnostic line and in
    /// the returned reason; the caller needs no other reaction.
    pub(crate) fn submit(&self, row: ScopeObservation) -> Result<(), DropReason> {
        let sent = match self
            .sender
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            Some(sender) => sender.try_send(Item::Row(Box::new(row))),
            None => return Err(report(DropReason::Closed)),
        };
        sent.map_err(|error| {
            report(match error {
                TrySendError::Full(_) => DropReason::QueueFull,
                TrySendError::Disconnected(_) => DropReason::Closed,
            })
        })
    }

    /// Stops accepting rows and lets the worker finish the queued ones.
    ///
    /// The first call waits briefly for the worker; rows still queued once the drain limit has
    /// passed are dropped, and the worker stops retrying a busy database. An error means the
    /// worker is still inside one write; call again later. The worker holds the storage log until
    /// it ends, so the database cannot close before this returns `Ok`.
    pub(crate) fn close(&self) -> io::Result<()> {
        let first = self
            .sender
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .is_some();
        if first {
            let _ = self
                .state
                .discard_after
                .set(Instant::now() + self.policy.drain_after_close);
            self.state.closing.store(true, Ordering::Release);
            self.waker.unpark();
        }
        let wait_until = Instant::now()
            + if first {
                self.policy.first_close_wait
            } else {
                Duration::ZERO
            };
        loop {
            {
                let mut worker = self.worker.lock().unwrap_or_else(PoisonError::into_inner);
                match worker.as_ref() {
                    None => return Ok(()),
                    Some(handle) if handle.is_finished() => {
                        let _ = worker.take().map(JoinHandle::join);
                        return Ok(());
                    }
                    Some(_) => {}
                }
            }
            if Instant::now() >= wait_until {
                return Err(io::Error::other(
                    "scope observation writer is still storing",
                ));
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// Returns once every row queued before the call has been handled.
    #[cfg(test)]
    pub(crate) fn flush(&self) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let (done, waiting) = mpsc::sync_channel(1);
        if let Some(sender) = sender
            && sender.send(Item::Fence(done)).is_ok()
        {
            let _ = waiting.recv();
        }
    }
}

fn report(reason: DropReason) -> DropReason {
    eprintln!("scope_observation.dropped: {}", reason.reason());
    reason
}

fn run(
    receiver: &mpsc::Receiver<Item>,
    log: &dyn ScopeObservationLog,
    state: &State,
    pauses: &[Duration],
) {
    // The loop ends when the writer drops its sender and the queue is empty.
    for item in receiver {
        match item {
            Item::Row(row) => store(log, &row, state, pauses),
            #[cfg(test)]
            Item::Fence(done) => {
                let _ = done.send(());
            }
        }
    }
}

/// Writes one row, repeating only a busy attempt, and reports the final failure.
fn store(
    log: &dyn ScopeObservationLog,
    row: &ScopeObservation,
    state: &State,
    pauses: &[Duration],
) {
    let mut retries = 0;
    loop {
        if state.expired() {
            eprintln!("scope_observation.dropped: shutdown_deadline");
            return;
        }
        // A panic in the storage adapter ends this row only; later rows still get their turn.
        let result = catch_unwind(AssertUnwindSafe(|| log.append(row)))
            .unwrap_or(Err(ScopeObservationWriteError::Storage));
        let Err(error) = result else { return };
        if error == ScopeObservationWriteError::Busy
            && !state.closing()
            && let Some(pause) = pauses.get(retries)
        {
            retries += 1;
            state.pause(*pause);
            continue;
        }
        eprintln!(
            "scope_observation.record_failed: {}",
            Failure::Store(error).reason()
        );
        return;
    }
}

#[cfg(test)]
mod tests;
