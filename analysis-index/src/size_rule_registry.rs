//! Composition-time registration of size aggregation rules by ID and revision.
//!
//! The assembling caller builds one registry, pins the latest revision of the chosen
//! ID when an analysis starts, and binds the result to the build before its first
//! observation. A result version then reports the identity it was built with.
//! Rules are dispatched statically; the registry only maps identities to them.
pub use crate::aggregation_rule::DEFAULT_SIZE_RULE_ID;
use crate::aggregation_rule::{BUILTIN_RULES, BoundSizeRule};
use crate::{BuildError, BuildHandle, SizeRuleIdentity};
pub use du_core_contracts::size_rule::SIZE_RULE_KIND;

#[cfg(test)]
mod tests;

/// Composition-time failure while assembling a [`SizeRuleRegistry`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeRuleRegistryError {
    /// The same ID and revision was registered twice.
    DuplicateRule,
    /// No revision of [`DEFAULT_SIZE_RULE_ID`] was registered.
    DefaultRuleMissing,
}

/// Collects rule registrations once, at the composition boundary.
#[derive(Debug, Default)]
pub struct SizeRuleRegistryBuilder {
    /// Sorted by identity, which keeps lookups and listings deterministic.
    rules: Vec<BoundSizeRule>,
}

impl SizeRuleRegistryBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the default bundle: every size rule this crate implements.
    ///
    /// # Errors
    /// Returns [`SizeRuleRegistryError::DuplicateRule`] if any of them is already
    /// registered, for example when the bundle is registered twice. Nothing is
    /// added in that case.
    pub fn register_defaults(&mut self) -> Result<(), SizeRuleRegistryError> {
        let bundle: Vec<BoundSizeRule> = BUILTIN_RULES
            .iter()
            .map(|r| BoundSizeRule::bind(*r))
            .collect();
        if bundle
            .iter()
            .any(|rule| self.position(rule.identity()).is_ok())
        {
            return Err(SizeRuleRegistryError::DuplicateRule);
        }
        for rule in bundle {
            self.register(rule)?;
        }
        Ok(())
    }

    /// Registers one rule without replacing an existing ID and revision.
    pub(crate) fn register(&mut self, rule: BoundSizeRule) -> Result<(), SizeRuleRegistryError> {
        match self.position(rule.identity()) {
            Ok(_) => Err(SizeRuleRegistryError::DuplicateRule),
            Err(at) => {
                self.rules.insert(at, rule);
                Ok(())
            }
        }
    }

    fn position(&self, identity: SizeRuleIdentity) -> Result<usize, usize> {
        self.rules
            .binary_search_by(|rule| rule.identity().cmp(&identity))
    }

    /// Freezes the registry.
    ///
    /// # Errors
    /// Returns [`SizeRuleRegistryError::DefaultRuleMissing`] unless some revision of
    /// [`DEFAULT_SIZE_RULE_ID`] is registered.
    pub fn build(self) -> Result<SizeRuleRegistry, SizeRuleRegistryError> {
        let rules = self.rules.into_boxed_slice();
        let default = latest_of(&rules, DEFAULT_SIZE_RULE_ID)
            .ok_or(SizeRuleRegistryError::DefaultRuleMissing)?;
        Ok(SizeRuleRegistry { rules, default })
    }
}

/// Immutable registrations of size aggregation rules.
#[derive(Debug)]
pub struct SizeRuleRegistry {
    rules: Box<[BoundSizeRule]>,
    default: BoundSizeRule,
}

fn latest_of(rules: &[BoundSizeRule], id: &str) -> Option<BoundSizeRule> {
    let end = rules.partition_point(|rule| rule.identity().id <= id);
    rules[..end]
        .last()
        .filter(|rule| rule.identity().id == id)
        .copied()
}

impl SizeRuleRegistry {
    /// Returns the highest registered revision of `id`, or `None` for an unknown ID.
    ///
    /// Callers pin this value when an analysis starts and treat `None` as an error.
    pub fn latest(&self, id: &str) -> Option<BoundSizeRule> {
        latest_of(&self.rules, id)
    }

    /// Returns exactly the requested revision, without falling back to another one.
    pub fn resolve(&self, id: &str, revision: u32) -> Option<BoundSizeRule> {
        self.rules
            .binary_search_by(|rule| {
                let identity = rule.identity();
                identity.id.cmp(id).then(identity.revision.cmp(&revision))
            })
            .ok()
            .map(|at| self.rules[at])
    }

    /// Returns the latest revision of [`DEFAULT_SIZE_RULE_ID`].
    pub fn default_rule(&self) -> BoundSizeRule {
        self.default
    }

    /// Lists the registered identities ordered by ID, then revision.
    pub fn identities(&self) -> impl Iterator<Item = SizeRuleIdentity> + '_ {
        self.rules.iter().map(|rule| rule.identity())
    }
}

impl BuildHandle {
    /// Binds the size aggregation rule used for every publication of this build.
    ///
    /// Call it before the first observation and before any publication. A build
    /// that binds nothing uses revision 1 of [`DEFAULT_SIZE_RULE_ID`].
    ///
    /// # Errors
    /// Returns [`BuildError::Unavailable`] once an observation was added, a
    /// version was published or the build finished. A replacement build keeps the
    /// rule of its base version and also refuses a new one.
    pub fn set_size_rule(&mut self, rule: BoundSizeRule) -> Result<(), BuildError> {
        if self.finished
            || self.published
            || !self.observations.is_empty()
            || self.expected_base.is_some()
        {
            return Err(BuildError::Unavailable);
        }
        self.size_rule = rule;
        Ok(())
    }
}
