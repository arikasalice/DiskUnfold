use std::collections::{HashMap, hash_map::Entry};

use serde_json::Value;

use crate::{
    contracts::{CallerCtx, Channel, UseCase},
    error::EngineError,
};

pub(crate) trait RegisteredUseCase: Send + Sync {
    fn channels(&self) -> &'static [Channel];
    fn invoke(&self, body: Value, caller: &CallerCtx) -> Result<Value, EngineError>;
}

impl<U: UseCase> RegisteredUseCase for U {
    fn channels(&self) -> &'static [Channel] {
        U::CHANNELS
    }

    fn invoke(&self, body: Value, caller: &CallerCtx) -> Result<Value, EngineError> {
        // Request contracts use object bodies on every channel. Check the container
        // before Serde, which also accepts positional arrays for Rust structs.
        if !body.is_object() {
            return Err(EngineError::reason("command.body_invalid"));
        }
        let request = serde_json::from_value(body)
            .map_err(|_| EngineError::reason("command.body_invalid"))?;
        self.check_scope(&request, caller)?;
        let response = self.execute(request, caller)?;
        serde_json::to_value(response).map_err(|_| EngineError::reason("support.internal_error"))
    }
}

#[derive(Default)]
pub(crate) struct Registry(HashMap<&'static str, Box<dyn RegisteredUseCase>>);

impl Registry {
    pub(crate) fn insert<U: UseCase>(&mut self, use_case: U) -> bool {
        let Entry::Vacant(entry) = self.0.entry(U::NAME) else {
            return false;
        };
        entry.insert(Box::new(use_case));
        true
    }

    pub(crate) fn get(&self, name: &str) -> Option<&dyn RegisteredUseCase> {
        self.0.get(name).map(Box::as_ref)
    }

    /// Lists every registered command with its allowed channels, ordered by name.
    #[cfg(test)]
    pub(crate) fn commands(&self) -> Vec<(&'static str, &'static [Channel])> {
        let mut commands: Vec<_> = self
            .0
            .iter()
            .map(|(name, use_case)| (*name, use_case.channels()))
            .collect();
        commands.sort_by_key(|(name, _)| *name);
        commands
    }
}
