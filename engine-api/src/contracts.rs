use std::{
    sync::{
        Arc,
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
}

impl CallerCtx {
    pub fn new(channel: Channel, correlation_id: Id, deadline: Option<Instant>) -> Self {
        Self {
            channel,
            correlation_id,
            deadline,
            cancellation: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancellation.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.load(Ordering::Acquire)
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
