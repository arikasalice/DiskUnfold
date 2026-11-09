use du_core_contracts::{CallEnvelope, ResultEnvelope};

use crate::{pipeline, registry::Registry};

pub use crate::contracts::{CallerCtx, Channel, OutboundFilter, UseCase};

#[derive(Debug, PartialEq, Eq)]
pub struct BuildError {
    pub duplicate_name: &'static str,
}

#[derive(Default)]
pub struct EngineBuilder {
    registry: Registry,
    duplicate_name: Option<&'static str>,
    outbound_filter: Option<Box<dyn OutboundFilter>>,
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

    pub fn build(self) -> Result<Engine, BuildError> {
        if let Some(duplicate_name) = self.duplicate_name {
            return Err(BuildError { duplicate_name });
        }
        Ok(Engine {
            registry: self.registry,
            outbound_filter: self.outbound_filter,
        })
    }
}

pub struct Engine {
    registry: Registry,
    outbound_filter: Option<Box<dyn OutboundFilter>>,
}

impl Engine {
    pub fn call(&self, envelope: CallEnvelope, caller: &CallerCtx) -> ResultEnvelope {
        pipeline::call(
            &self.registry,
            self.outbound_filter.as_deref(),
            envelope,
            caller,
        )
    }
}
