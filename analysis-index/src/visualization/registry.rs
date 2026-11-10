//! Composition-time registration and exact selection of visualization strategies.
use du_core_contracts::visualization::{
    LayoutDescriptor as Descriptor, LayoutStrategyKey as StrategyKey,
};
use std::collections::BTreeMap;

/// Invalid or missing configuration at composition time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationError {
    InvalidDescriptor,
    DuplicateStrategy,
    /// A format and strategy ID must refer to each other across all revisions.
    ConflictingFormat,
    RequiredStrategyMissing,
}

/// Invalid selection at request admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionError {
    UnknownFormat,
    UnknownStrategy,
}

/// One strategy bound to its registered format and revision.
pub struct Registered<S> {
    pub descriptor: Descriptor,
    pub implementation: S,
}

/// Populates the registry once at the composition boundary.
pub struct Builder<S>(BTreeMap<StrategyKey, Registered<S>>);

impl<S> Default for Builder<S> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}

impl<S> Builder<S> {
    /// Registers a new key without replacing a previous revision or another format.
    pub fn register(
        &mut self,
        descriptor: Descriptor,
        implementation: S,
    ) -> Result<(), RegistrationError> {
        let key = &descriptor.strategy;
        if descriptor.format.trim().is_empty() || key.id.trim().is_empty() || key.revision == 0 {
            return Err(RegistrationError::InvalidDescriptor);
        }
        if self.0.contains_key(key) {
            return Err(RegistrationError::DuplicateStrategy);
        }
        if self.0.values().any(|entry| {
            (entry.descriptor.format == descriptor.format)
                != (entry.descriptor.strategy.id == key.id)
        }) {
            return Err(RegistrationError::ConflictingFormat);
        }
        self.0.insert(
            key.clone(),
            Registered {
                descriptor,
                implementation,
            },
        );
        Ok(())
    }

    /// Freezes a registry after checking that all required format/strategy pairs are present.
    pub fn build(self, required: &[Descriptor]) -> Result<Registry<S>, RegistrationError> {
        for descriptor in required {
            if self
                .0
                .get(&descriptor.strategy)
                .is_none_or(|entry| entry.descriptor != *descriptor)
            {
                return Err(RegistrationError::RequiredStrategyMissing);
            }
        }
        Ok(Registry(self.0))
    }
}

/// Immutable registrations for catalog enumeration and versioned dispatch.
pub struct Registry<S>(BTreeMap<StrategyKey, Registered<S>>);

impl<S> Registry<S> {
    /// Lists supported format/strategy pairs in stable strategy-key order.
    pub fn catalog(&self) -> impl Iterator<Item = &Descriptor> {
        self.0.values().map(|entry| &entry.descriptor)
    }

    /// Resolves exactly the requested format and revision without fallback.
    pub fn resolve(
        &self,
        format: &str,
        key: &StrategyKey,
    ) -> Result<&Registered<S>, SelectionError> {
        if let Some(entry) = self
            .0
            .get(key)
            .filter(|entry| entry.descriptor.format == format)
        {
            return Ok(entry);
        }
        if self.catalog().any(|entry| entry.format == format) {
            Err(SelectionError::UnknownStrategy)
        } else {
            Err(SelectionError::UnknownFormat)
        }
    }

    /// Selects the highest registered revision only when the caller omits a strategy key.
    pub fn latest(&self, format: &str) -> Result<&Registered<S>, SelectionError> {
        self.0
            .values()
            .rev()
            .find(|entry| entry.descriptor.format == format)
            .ok_or(SelectionError::UnknownFormat)
    }
}
