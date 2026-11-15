//! Assemble the size aggregation rules and pin one when an analysis starts
//! (F03 FR14.HD2.app-host.DD3).
//!
//! The composition root builds one [`SizeRules`] value: the frozen registry of
//! analysis-index's default bundle plus the source that names the rule ID a new
//! analysis uses. `scan.start` reads the source once at admission, resolves the
//! latest revision of that ID and binds it to the scan, so a running scan keeps
//! its rule while a changed source applies from the next start.
use std::{fmt, sync::Arc};

use du_analysis_index::{
    BoundSizeRule, SizeRuleIdentity,
    size_rule_registry::{SizeRuleRegistry, SizeRuleRegistryError},
};

pub use du_analysis_index::size_rule_registry::SizeRuleRegistryBuilder;
pub use du_core_contracts::size_rule::{DEFAULT_SIZE_RULE_ID, SPACE_RULE_UNKNOWN};

use crate::error::EngineError;

#[cfg(test)]
mod tests;

/// Names the size aggregation rule ID for the next analysis.
///
/// The source is read once for each admitted `scan.start`; an admitted scan
/// keeps the rule it resolved. Any ID that is not registered is rejected with the
/// reason `space.rule_unknown`.
pub type SizeRuleSource = Arc<dyn Fn() -> String + Send + Sync>;

/// Returns a source that always names [`DEFAULT_SIZE_RULE_ID`].
pub fn default_size_rule_source() -> SizeRuleSource {
    Arc::new(|| DEFAULT_SIZE_RULE_ID.to_owned())
}

/// Composition-time failure while assembling the size aggregation rules.
///
/// The composition root treats it as a failed registration and stops startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeRuleSetupError {
    /// The same rule ID and revision was registered twice, for example because the
    /// default bundle was registered twice into one builder.
    DuplicateRule,
    /// No revision of [`DEFAULT_SIZE_RULE_ID`] was registered.
    DefaultRuleMissing,
}

impl fmt::Display for SizeRuleSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DuplicateRule => "size rule registered twice",
            Self::DefaultRuleMissing => "default size rule not registered",
        })
    }
}

impl std::error::Error for SizeRuleSetupError {}

impl From<SizeRuleRegistryError> for SizeRuleSetupError {
    fn from(error: SizeRuleRegistryError) -> Self {
        match error {
            SizeRuleRegistryError::DuplicateRule => Self::DuplicateRule,
            SizeRuleRegistryError::DefaultRuleMissing => Self::DefaultRuleMissing,
        }
    }
}

/// Registers analysis-index's default bundle of size rules into `builder`.
///
/// Call it once for each builder.
///
/// # Errors
/// Returns [`SizeRuleSetupError::DuplicateRule`] if the bundle is already
/// registered in `builder`. Nothing is added in that case.
pub fn register_default_size_rules(
    builder: &mut SizeRuleRegistryBuilder,
) -> Result<(), SizeRuleSetupError> {
    builder.register_defaults().map_err(Into::into)
}

/// The frozen size rule registry and the source that selects a rule ID.
///
/// Cloning shares the registry and the source.
#[derive(Clone)]
pub struct SizeRules {
    registry: Arc<SizeRuleRegistry>,
    source: SizeRuleSource,
}

impl SizeRules {
    /// Freezes `builder` and pairs it with `source`.
    ///
    /// # Errors
    /// Returns [`SizeRuleSetupError::DefaultRuleMissing`] unless some revision of
    /// [`DEFAULT_SIZE_RULE_ID`] was registered.
    pub fn assemble(
        builder: SizeRuleRegistryBuilder,
        source: SizeRuleSource,
    ) -> Result<Self, SizeRuleSetupError> {
        Ok(Self {
            registry: Arc::new(builder.build()?),
            source,
        })
    }

    /// Registers the default bundle once into a new registry and pairs it with `source`.
    ///
    /// This is the one call a composition root makes. A registry that cannot be
    /// assembled must stop application start.
    ///
    /// # Errors
    /// Returns the [`SizeRuleSetupError`] of the registration or of freezing.
    pub fn default_bundle(source: SizeRuleSource) -> Result<Self, SizeRuleSetupError> {
        let mut builder = SizeRuleRegistryBuilder::new();
        register_default_size_rules(&mut builder)?;
        Self::assemble(builder, source)
    }

    /// Lists the registered rule identities ordered by ID, then revision.
    pub fn identities(&self) -> Vec<SizeRuleIdentity> {
        self.registry.identities().collect()
    }

    /// Reads the source once and resolves the latest revision of the ID it names.
    ///
    /// The result is pinned for one scan. An ID that is not registered yields
    /// `space.rule_unknown`, and the caller must create no job.
    pub(crate) fn bind_latest(&self) -> Result<BoundSizeRule, EngineError> {
        let id = (self.source)();
        self.registry
            .latest(&id)
            .ok_or_else(|| EngineError::reason(SPACE_RULE_UNKNOWN))
    }
}
