use crate::delivery::enqueue;
use crate::model::*;
use du_core_contracts::Id;
use std::{
    cell::Cell,
    sync::{Mutex, mpsc},
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// A non-blocking shutdown acknowledgement. Poll until completion or the deadline.
pub struct ShutdownTicket {
    receiver: mpsc::Receiver<usize>,
    consumer: Mutex<Option<JoinHandle<()>>>,
    started_at: Instant,
    timeout: Duration,
    status: Cell<Option<ShutdownStatus>>,
    history_failures: Cell<Option<usize>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownStatus {
    Pending,
    Complete,
    TimedOut,
    Failed,
}

/// One job's shutdown cancellation result, paired with its F24 job reference.
pub type ShutdownCancellation = (Id, Result<(CancelResult, u64), RuntimeError>);

/// A non-blocking acknowledgement that the delivery queue passed a fence.
///
/// Everything queued before the fence, including the terminal history of every
/// job that already finished, has been delivered when the flush completes.
/// Jobs that are still running are not waited for and the runtime stays open.
pub struct DeliveryFlush {
    receiver: mpsc::Receiver<()>,
    started_at: Instant,
    timeout: Duration,
    status: Cell<Option<ShutdownStatus>>,
}

impl DeliveryFlush {
    /// Polls without waiting. `TimedOut` may be followed by `Complete` on a later poll.
    pub fn poll(&self) -> ShutdownStatus {
        if let Some(status) = self.status.get() {
            return status;
        }
        match self.receiver.try_recv() {
            Ok(()) => {
                self.status.set(Some(ShutdownStatus::Complete));
                ShutdownStatus::Complete
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status.set(Some(ShutdownStatus::Failed));
                ShutdownStatus::Failed
            }
            Err(mpsc::TryRecvError::Empty) if self.started_at.elapsed() >= self.timeout => {
                ShutdownStatus::TimedOut
            }
            Err(mpsc::TryRecvError::Empty) => ShutdownStatus::Pending,
        }
    }
}

impl ShutdownTicket {
    pub fn poll(&self) -> ShutdownStatus {
        if let Some(status) = self.status.get() {
            return status;
        }
        match self.receiver.try_recv() {
            Ok(history_failures) => {
                self.history_failures.set(Some(history_failures));
                let joined = self.consumer.lock().is_ok_and(|mut consumer| {
                    consumer
                        .take()
                        .is_none_or(|consumer| consumer.join().is_ok())
                });
                let status = if joined && history_failures == 0 {
                    ShutdownStatus::Complete
                } else {
                    ShutdownStatus::Failed
                };
                self.status.set(Some(status));
                status
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                let _ = self.consumer.lock().is_ok_and(|mut consumer| {
                    consumer
                        .take()
                        .is_none_or(|consumer| consumer.join().is_ok())
                });
                self.status.set(Some(ShutdownStatus::Failed));
                ShutdownStatus::Failed
            }
            Err(mpsc::TryRecvError::Empty) if self.started_at.elapsed() >= self.timeout => {
                ShutdownStatus::TimedOut
            }
            Err(mpsc::TryRecvError::Empty) => ShutdownStatus::Pending,
        }
    }

    /// Returns the failed history append count after the delivery barrier completes.
    pub fn history_failures(&self) -> Option<usize> {
        self.history_failures.get()
    }
}
impl JobRuntime {
    /// Queues a fence behind everything already awaiting delivery, without
    /// requiring jobs to be terminal and without stopping the consumer.
    ///
    /// Use this before abandoning jobs that will never finish, so the terminal
    /// history of jobs that did finish is not lost with the process.
    pub fn begin_delivery_flush(&self, timeout: Duration) -> Result<DeliveryFlush, RuntimeError> {
        let (sender, receiver) = mpsc::channel();
        let table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        enqueue(&table, Delivery::Fence(sender))?;
        drop(table);
        Ok(DeliveryFlush {
            receiver,
            started_at: Instant::now(),
            timeout,
            status: Cell::new(None),
        })
    }

    /// Stop accepting jobs after all jobs terminate. Poll the ticket without blocking a UI thread.
    pub fn begin_shutdown(&self, timeout: Duration) -> Result<ShutdownTicket, RuntimeError> {
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        if table.shutdown_delivery_started {
            return Err(RuntimeError::ShuttingDown);
        }
        if table.jobs.values().any(|job| !job.state.is_terminal()) {
            return Err(RuntimeError::ActiveJobs);
        }
        let (sender, receiver) = mpsc::channel();
        enqueue(&table, Delivery::Stop(sender))?;
        table.shutting_down = true;
        table.shutdown_delivery_started = true;
        drop(table);
        self.flow_limits.wake_waiters();
        let consumer = self
            .consumer
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?
            .take();
        Ok(ShutdownTicket {
            receiver,
            consumer: Mutex::new(consumer),
            started_at: Instant::now(),
            timeout,
            status: Cell::new(None),
            history_failures: Cell::new(None),
        })
    }

    /// Requests F24 cancellation for every nonterminal job in submission order
    /// after [`JobRuntime::close_admission_for_shutdown`] has accepted shutdown.
    /// The runtime stays alive so active bodies can reach their safe points and
    /// commit their terminal history normally.
    ///
    /// Per-job control errors are returned with their job reference so one failed
    /// cancellation delivery does not skip the remaining jobs. New submissions
    /// and worker starts are rejected before the first cancellation is sent.
    pub fn cancel_all_for_shutdown(&self) -> Result<Vec<ShutdownCancellation>, RuntimeError> {
        let job_refs = {
            let now = self.clock.now_ms();
            let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
            if !table.shutting_down {
                return Err(RuntimeError::InvalidState);
            }
            expire(&mut table, now);
            let mut jobs: Vec<_> = table
                .jobs
                .iter()
                .filter(|(_, job)| !job.state.is_terminal())
                .map(|(id, job)| (job.order, id.clone()))
                .collect();
            jobs.sort_unstable_by_key(|(order, _)| *order);
            jobs.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
        };
        self.flow_limits.wake_waiters();

        Ok(job_refs
            .into_iter()
            .map(|id| {
                let result = self.cancel_with_sequence(&id);
                (id, result)
            })
            .collect())
    }
}
impl Drop for JobRuntime {
    fn drop(&mut self) {
        let table = self
            .table
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !table.shutting_down {
            let (sender, _) = mpsc::channel();
            let _ = enqueue(&table, Delivery::Stop(sender));
        }
        // Detach: a blocked external port must not block Drop.
        let consumer = self
            .consumer
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        consumer.take();
    }
}
