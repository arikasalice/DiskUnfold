use crate::delivery::enqueue;
use crate::model::*;
use std::{
    cell::Cell,
    sync::mpsc,
    time::{Duration, Instant},
};

/// A non-blocking shutdown acknowledgement. Poll until completion or the deadline.
pub struct ShutdownTicket {
    receiver: mpsc::Receiver<()>,
    started_at: Instant,
    timeout: Duration,
    complete: Cell<bool>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownStatus {
    Pending,
    Complete,
    TimedOut,
    Failed,
}
impl ShutdownTicket {
    pub fn poll(&self) -> ShutdownStatus {
        if self.complete.get() {
            return ShutdownStatus::Complete;
        }
        match self.receiver.try_recv() {
            Ok(()) => {
                self.complete.set(true);
                ShutdownStatus::Complete
            }
            Err(mpsc::TryRecvError::Disconnected) => ShutdownStatus::Failed,
            Err(mpsc::TryRecvError::Empty) if self.started_at.elapsed() >= self.timeout => {
                ShutdownStatus::TimedOut
            }
            Err(mpsc::TryRecvError::Empty) => ShutdownStatus::Pending,
        }
    }
}
impl JobRuntime {
    /// Stop accepting jobs after all jobs terminate. Poll the ticket without blocking a UI thread.
    pub fn begin_shutdown(&self, timeout: Duration) -> Result<ShutdownTicket, RuntimeError> {
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        if table.shutting_down {
            return Err(RuntimeError::ShuttingDown);
        }
        if table.jobs.values().any(|job| !job.state.is_terminal()) {
            return Err(RuntimeError::ActiveJobs);
        }
        let (sender, receiver) = mpsc::channel();
        enqueue(&table, Delivery::Stop(sender))?;
        table.shutting_down = true;
        drop(table);
        self.flow_limits.wake_waiters();
        Ok(ShutdownTicket {
            receiver,
            started_at: Instant::now(),
            timeout,
            complete: Cell::new(false),
        })
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
