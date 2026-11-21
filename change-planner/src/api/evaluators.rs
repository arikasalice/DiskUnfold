use crate::{domain, ports::EvaluatorId, ports::ProtectionEvaluator};
use std::collections::BTreeMap;

/// Registry of the fixed F10 protection evaluator set.
pub struct EvaluatorRegistry {
    entries: BTreeMap<EvaluatorId, Box<dyn ProtectionEvaluator>>,
}

impl EvaluatorRegistry {
    /// Creates an empty registry for assembly and deterministic tests.
    pub fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Creates the twelve built-in evaluators in architecture order.
    pub fn standard() -> Self {
        let entries = domain::standard_evaluators()
            .map(|evaluator| (evaluator.id(), evaluator))
            .collect();
        Self { entries }
    }

    /// Registers an evaluator, rejecting a duplicate stable identifier.
    pub fn register(
        &mut self,
        evaluator: Box<dyn ProtectionEvaluator>,
    ) -> Result<(), EvaluatorRegistryError> {
        let id = evaluator.id();
        if self.entries.contains_key(&id) {
            return Err(EvaluatorRegistryError::Duplicate(id));
        }
        self.entries.insert(id, evaluator);
        Ok(())
    }

    /// Verifies that all twelve required IDs are present exactly once.
    pub fn validate(&self) -> Result<(), EvaluatorRegistryError> {
        for id in EvaluatorId::ORDERED {
            if !self.entries.contains_key(&id) {
                return Err(EvaluatorRegistryError::Missing(id));
            }
        }
        Ok(())
    }

    pub(crate) fn ordered(&self) -> Vec<&dyn ProtectionEvaluator> {
        EvaluatorId::ORDERED
            .into_iter()
            .filter_map(|id| self.entries.get(&id).map(Box::as_ref))
            .collect()
    }
}

/// Invalid evaluator registration discovered during assembly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluatorRegistryError {
    /// An ID was registered more than once.
    Duplicate(EvaluatorId),
    /// A required evaluator ID was not registered.
    Missing(EvaluatorId),
}
