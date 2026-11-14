//! F36 rule storage port over the shared GUI database connection.
use std::sync::Arc;

use du_core_contracts::{
    group_rules::{
        GroupRulesReadError, GroupRulesSaveOutcome, GroupRulesStore, StoredGroupRuleset,
    },
    startup::StartupCapabilitiesPort,
};
use du_engine_api::{api::EngineBuilder, group::GroupAssembly};

use crate::shared_database::SharedDatabase;

/// Serializes rule reads and replacements with the other shared-connection writers.
struct SharedGroupRules(SharedDatabase);

impl GroupRulesStore for SharedGroupRules {
    fn read(&self) -> Result<Option<StoredGroupRuleset>, GroupRulesReadError> {
        self.0
            .with_connection(|database| database.read_group_ruleset())
            .unwrap_or(Err(GroupRulesReadError::Unavailable))
    }

    fn replace(&self, expected_revision: u32, next: &StoredGroupRuleset) -> GroupRulesSaveOutcome {
        self.0
            .with_connection(|database| database.replace_group_ruleset(expected_revision, next))
            .unwrap_or(GroupRulesSaveOutcome::Failed)
    }
}

/// Registers the F36 group use cases with a rule store on the shared GUI connection.
/// A registration failure is reported before the window opens.
pub(crate) fn register(
    builder: &mut EngineBuilder,
    groups: GroupAssembly,
    database: &SharedDatabase,
    capabilities: &dyn StartupCapabilitiesPort,
) -> Result<(), std::io::Error> {
    let rules = Arc::new(SharedGroupRules(database.clone()));
    groups.register(builder, rules).map_err(|error| {
        crate::startup_checks::report_pre_window_error(
            capabilities,
            std::io::Error::other(format!("startup.registration_failed: {error:?}")),
        )
    })
}

impl SharedDatabase {
    /// Runs one short storage call on the shared connection; `None` if it is poisoned.
    fn with_connection<T>(&self, call: impl FnOnce(&mut du_app_db::AppDb) -> T) -> Option<T> {
        let mut connection = self.0.connection.lock().ok()?;
        Some(call(&mut connection))
    }
}
