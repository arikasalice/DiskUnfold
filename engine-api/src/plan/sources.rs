//! External persistence and fact adapters used by the desktop assembly.

use super::storage::RulesetAdapter as RawRulesetAdapter;
use super::{
    DomainPlatformFactsPort, PlanAnalysisSource, ProtectionRulesetPort, SharedVersionStore,
    TargetCheckPort,
};
use std::sync::Arc;

/// External persistence and optional fact adapters used by the desktop assembly.
pub struct PlanSources {
    pub(super) versions: SharedVersionStore,
    pub(super) rulesets: Arc<dyn ProtectionRulesetPort>,
    pub(super) analysis: Option<Arc<dyn PlanAnalysisSource>>,
    pub(super) target_check: Option<Arc<dyn TargetCheckPort>>,
    pub(super) platform: Option<Arc<dyn DomainPlatformFactsPort>>,
}

/// Creates the canonical built-in F29 v1 ruleset row for GUI startup.
pub fn built_in_protection_ruleset_row(
    created_at_ms: i64,
) -> Result<du_core_contracts::ruleset::RulesetRow, du_core_contracts::ruleset::RulesetReadError> {
    du_change_planner::ruleset::built_in_v1_row(created_at_ms)
}

impl PlanSources {
    /// Builds production adapters from the existing app-db ports.
    pub fn new(
        versions: SharedVersionStore,
        rulesets: Arc<dyn du_core_contracts::ruleset::RulesetRead + Send + Sync>,
    ) -> Self {
        Self {
            versions,
            rulesets: Arc::new(RawRulesetAdapter::new(rulesets)),
            analysis: None,
            target_check: None,
            platform: None,
        }
    }

    /// Uses injected analysis facts, as in deterministic app-host boundary tests.
    pub fn with_analysis(mut self, analysis: Arc<dyn PlanAnalysisSource>) -> Self {
        self.analysis = Some(analysis);
        self
    }

    /// Uses injected platform and target-check facts for deterministic tests.
    pub fn with_fact_ports(
        mut self,
        target_check: Arc<dyn TargetCheckPort>,
        platform: Arc<dyn DomainPlatformFactsPort>,
    ) -> Self {
        self.target_check = Some(target_check);
        self.platform = Some(platform);
        self
    }

    /// Uses the shared platform protection facts contract and maps it into planner facts.
    pub fn with_protection_facts(
        mut self,
        facts: Arc<dyn du_core_contracts::protection_facts::ProtectionFactsPort>,
    ) -> Self {
        self.platform = Some(Arc::new(
            super::protection_facts::ProtectionFactsAdapter::new(facts),
        ));
        self
    }

    /// Uses an already validated ruleset snapshot source.
    pub fn with_ruleset_port(mut self, rulesets: Arc<dyn ProtectionRulesetPort>) -> Self {
        self.rulesets = rulesets;
        self
    }
}
