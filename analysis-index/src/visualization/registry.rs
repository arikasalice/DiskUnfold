//! Composition-time registration and exact selection of visualization strategies.
use du_core_contracts::visualization::{LayoutDescriptor as Descriptor, LayoutStrategyKey as StrategyKey};
use std::collections::{BTreeMap, btree_map::Entry};

/// Invalid or missing configuration at composition time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationError {
    InvalidDescriptor,
    DuplicateStrategy,
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
        match self.0.entry(key.clone()) {
            Entry::Occupied(_) => Err(RegistrationError::DuplicateStrategy),
            Entry::Vacant(slot) => {
                slot.insert(Registered {
                    descriptor,
                    implementation,
                });
                Ok(())
            }
        }
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
}
