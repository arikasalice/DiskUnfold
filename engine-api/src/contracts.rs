use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use du_core_contracts::Id;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::error::EngineError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Gui,
    Cli,
    Mcp,
}

#[derive(Clone)]
pub struct CallerCtx {
    pub channel: Channel,
    pub correlation_id: Id,
    pub deadline: Option<Instant>,
    cancellation: Arc<AtomicBool>,
    cancellation_fence: Arc<Mutex<()>>,
}

impl CallerCtx {
    pub fn new(channel: Channel, correlation_id: Id, deadline: Option<Instant>) -> Self {
        Self {
            channel,
            correlation_id,
            deadline,
            cancellation: Arc::new(AtomicBool::new(false)),
            cancellation_fence: Arc::new(Mutex::new(())),
        }
    }

    pub fn cancel(&self) {
        let _fence = self.cancellation_fence.lock();
        self.cancellation.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.load(Ordering::Acquire)
    }

    /// Runs a short in-memory commit while request cancellation is serialized.
    pub(crate) fn with_active<T>(&self, action: impl FnOnce() -> T) -> Option<T> {
        let _fence = self.cancellation_fence.lock().ok()?;
        if self.is_cancelled() {
            return None;
        }
        Some(action())
    }
}

/// Requests define their allowed fields with `#[serde(deny_unknown_fields)]`.
pub trait UseCase: Send + Sync + 'static {
    const NAME: &'static str;
    const CHANNELS: &'static [Channel];
    type Request: DeserializeOwned;
    type Response: Serialize;

    fn check_scope(&self, request: &Self::Request, caller: &CallerCtx) -> Result<(), EngineError>;

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError>;
}

/// Outbound data must pass through the composed sharing policy for CLI and MCP.
pub trait OutboundFilter: Send + Sync + 'static {
    fn filter(&self, name: &str, caller: &CallerCtx, response: Value)
    -> Result<Value, EngineError>;
}

#[cfg(test)]
mod tests {
    use super::{CallerCtx, Channel};
    use du_core_contracts::Id;
    use std::{
        sync::{Arc, Barrier, mpsc},
        thread,
        time::Duration,
    };

    fn id() -> Id {
        let mut bytes = [0; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = 1;
        Id::from_v7_bytes(bytes)
    }

    #[test]
    fn cancellation_waits_for_an_active_commit_fence() -> Result<(), String> {
        let caller = CallerCtx::new(Channel::Gui, id(), None);
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let active = {
            let caller = caller.clone();
            let entered = entered.clone();
            let release = release.clone();
            thread::spawn(move || {
                caller.with_active(|| {
                    entered.wait();
                    release.wait();
                    7
                })
            })
        };
        entered.wait();

        let (started_tx, started_rx) = mpsc::channel();
        let (cancelled_tx, cancelled_rx) = mpsc::channel();
        let cancel = {
            let caller = caller.clone();
            thread::spawn(move || {
                started_tx.send(()).map_err(|_| "start receiver closed")?;
                caller.cancel();
                cancelled_tx
                    .send(())
                    .map_err(|_| "cancel receiver closed")?;
                Ok::<(), &'static str>(())
            })
        };
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| error.to_string())?;
        assert!(
            cancelled_rx
                .recv_timeout(Duration::from_millis(20))
                .is_err()
        );
        release.wait();
        assert_eq!(
            active
                .join()
                .map_err(|_| "commit thread panicked".to_owned())?,
            Some(7)
        );
        cancelled_rx
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| error.to_string())?;
        cancel
            .join()
            .map_err(|_| "cancel thread panicked".to_owned())?
            .map_err(str::to_owned)?;
        assert!(caller.is_cancelled());
        Ok(())
    }

    #[test]
    fn cancellation_before_commit_rejects_the_action() -> Result<(), String> {
        let caller = CallerCtx::new(Channel::Gui, id(), None);
        caller.cancel();
        assert_eq!(caller.with_active(|| ()), None);
        Ok(())
    }
}
