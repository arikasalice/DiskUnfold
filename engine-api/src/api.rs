use du_core_contracts::{CallEnvelope, ResultEnvelope};

use crate::{pipeline, registry::Registry};

pub use crate::contracts::{CallerCtx, Channel, OutboundFilter, UseCase};

#[derive(Debug, PartialEq, Eq)]
pub struct BuildError {
    pub duplicate_name: &'static str,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "duplicate use case: {}", self.duplicate_name)
    }
}

impl std::error::Error for BuildError {}

#[derive(Default)]
pub struct EngineBuilder {
    registry: Registry,
    duplicate_name: Option<&'static str>,
    outbound_filter: Option<Box<dyn OutboundFilter>>,
    session_retire: Option<Box<dyn Fn() + Send + Sync>>,
}

impl EngineBuilder {
    pub fn register<U: UseCase>(&mut self, use_case: U) {
        if !self.registry.insert(use_case) && self.duplicate_name.is_none() {
            self.duplicate_name = Some(U::NAME);
        }
    }

    pub fn with_outbound_filter(&mut self, filter: impl OutboundFilter) {
        self.outbound_filter = Some(Box::new(filter));
    }

    pub(crate) fn on_session_retire(&mut self, retire: impl Fn() + Send + Sync + 'static) {
        let previous = self.session_retire.take();
        self.session_retire = Some(Box::new(move || {
            if let Some(previous) = &previous {
                previous();
            }
            retire();
        }));
    }

    pub fn build(self) -> Result<Engine, BuildError> {
        if let Some(duplicate_name) = self.duplicate_name {
            return Err(BuildError { duplicate_name });
        }
        Ok(Engine {
            registry: self.registry,
            outbound_filter: self.outbound_filter,
            session_retire: self.session_retire,
        })
    }
}

pub struct Engine {
    registry: Registry,
    outbound_filter: Option<Box<dyn OutboundFilter>>,
    session_retire: Option<Box<dyn Fn() + Send + Sync>>,
}

impl Engine {
    /// Retires query regions when the owning desktop window session ends.
    pub fn retire_window_session(&self) {
        if let Some(retire) = &self.session_retire {
            retire();
        }
    }

    pub fn call(&self, envelope: CallEnvelope, caller: &CallerCtx) -> ResultEnvelope {
        pipeline::call(
            &self.registry,
            self.outbound_filter.as_deref(),
            envelope,
            caller,
        )
    }
}
