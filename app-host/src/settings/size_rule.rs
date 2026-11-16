//! Read the size aggregation rule setting when an analysis starts (F03 FR14.HD2.app-host.DD4).
use std::{io, sync::Arc};

use du_core_contracts::size_rule::DEFAULT_SIZE_RULE_ID;
use du_engine_api::size_rule::{SizeRuleSource, SizeRules, default_size_rule_source};
use du_user_env::space::AGGREGATION_RULE;
use serde_json::Value;

use super::StartupSettings;

/// Assembles the registry shared by settings validation and desktop scans.
pub(super) fn assemble() -> Result<SizeRules, io::Error> {
    SizeRules::default_bundle(default_size_rule_source())
        .map_err(|error| io::Error::other(format!("startup.registration_failed: {error}")))
}

/// Lists the IDs from the already assembled registry that settings may accept.
pub(super) fn registered_ids(rules: &SizeRules) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for identity in rules.identities() {
        if !ids.iter().any(|id| id == identity.id) {
            ids.push(identity.id.to_owned());
        }
    }
    ids
}

/// Names the rule an analysis uses: the stored ID when the registry has it, else the default.
///
/// The settings write path and the startup restore reject an unregistered ID, so a value
/// outside `registered` only reaches this point through a snapshot that bypassed them.
/// Falling back keeps `space.rule_unknown` out of normal use.
fn resolve(stored: Option<&Value>, registered: &[String]) -> String {
    stored
        .and_then(Value::as_str)
        .filter(|id| registered.iter().any(|known| known == id))
        .unwrap_or(DEFAULT_SIZE_RULE_ID)
        .to_owned()
}

impl StartupSettings {
    /// Shares the startup registry with scan execution and reads the setting at admission.
    pub(crate) fn size_rules(self: Arc<Self>) -> SizeRules {
        let source = self.clone().size_rule_source();
        self.size_rules.with_source(source)
    }

    /// Supplies the size aggregation rule ID when an analysis is admitted.
    ///
    /// The source reads the current `space.aggregation_rule` value on every call, so a
    /// changed value applies from the next analysis start while a running analysis keeps
    /// the rule it pinned at admission. An absent value, an unregistered ID or an
    /// unreadable service yields [`DEFAULT_SIZE_RULE_ID`].
    pub(crate) fn size_rule_source(self: Arc<Self>) -> SizeRuleSource {
        Arc::new(move || {
            self.service
                .lock()
                .ok()
                .map(|service| {
                    resolve(
                        service.snapshot().values.get(AGGREGATION_RULE),
                        &self.size_rule_ids,
                    )
                })
                .unwrap_or_else(|| DEFAULT_SIZE_RULE_ID.to_owned())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn registered() -> Vec<String> {
        vec![DEFAULT_SIZE_RULE_ID.to_owned(), "size.agg.other".to_owned()]
    }

    #[test]
    fn a_registered_id_is_kept() {
        assert_eq!(
            resolve(Some(&json!("size.agg.other")), &registered()),
            "size.agg.other"
        );
    }

    #[test]
    fn absent_unregistered_and_malformed_values_use_the_default() {
        for stored in [
            None,
            Some(json!("size.agg.removed")),
            Some(json!("")),
            Some(json!(7)),
            Some(json!(null)),
        ] {
            assert_eq!(
                resolve(stored.as_ref(), &registered()),
                DEFAULT_SIZE_RULE_ID
            );
        }
    }

    #[test]
    fn registered_ids_come_from_the_assembled_default_bundle() -> Result<(), io::Error> {
        let rules = assemble()?;
        assert_eq!(
            registered_ids(&rules),
            vec![DEFAULT_SIZE_RULE_ID.to_owned()]
        );
        Ok(())
    }

    #[test]
    fn a_poisoned_settings_lock_uses_the_default_rule() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = du_app_db::AppDb::open_or_create_gui(&directory.path().join("settings.db"))
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        let database = crate::shared_database::SharedDatabase::new(database, None);
        let settings = Arc::new(StartupSettings::open(&database, 1, &["en", "ko"])?);
        let source = settings.clone().size_rule_source();

        let panic = catch_unwind(AssertUnwindSafe(|| {
            if let Ok(_service) = settings.service.lock() {
                std::panic::resume_unwind(Box::new("intentional test unwind"));
            }
        }));
        assert!(panic.is_err());
        assert!(settings.service.is_poisoned());
        assert_eq!(source(), DEFAULT_SIZE_RULE_ID);
        Ok(())
    }
}
