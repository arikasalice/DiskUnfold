//! Assembly and pinning of size aggregation rules (F03 FR14.HD2.app-host.DD3).
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

fn source_of(id: &'static str, reads: Arc<AtomicUsize>) -> SizeRuleSource {
    Arc::new(move || {
        reads.fetch_add(1, Ordering::SeqCst);
        id.to_owned()
    })
}

#[test]
fn default_bundle_lists_exactly_the_default_rule() -> Result<(), SizeRuleSetupError> {
    let rules = SizeRules::default_bundle(default_size_rule_source())?;
    assert_eq!(
        rules.identities(),
        vec![SizeRuleIdentity {
            id: DEFAULT_SIZE_RULE_ID,
            revision: 1,
        }]
    );
    Ok(())
}

#[test]
fn registering_the_bundle_twice_reports_a_duplicate() {
    let mut builder = SizeRuleRegistryBuilder::new();
    assert_eq!(register_default_size_rules(&mut builder), Ok(()));
    assert_eq!(
        register_default_size_rules(&mut builder),
        Err(SizeRuleSetupError::DuplicateRule)
    );
}

#[test]
fn an_empty_registry_reports_the_missing_default() {
    let result = SizeRules::assemble(SizeRuleRegistryBuilder::new(), default_size_rule_source());
    assert_eq!(result.err(), Some(SizeRuleSetupError::DefaultRuleMissing));
}

#[test]
fn pin_reads_the_source_once_and_resolves_the_latest_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let reads = Arc::new(AtomicUsize::new(0));
    let rules = SizeRules::default_bundle(source_of(DEFAULT_SIZE_RULE_ID, reads.clone()))?;
    let pinned = rules.bind_latest().map_err(|error| format!("{error:?}"))?;
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(pinned.identity().id, DEFAULT_SIZE_RULE_ID);
    assert_eq!(pinned.identity().revision, 1);
    Ok(())
}

#[test]
fn pin_rejects_an_unregistered_id_with_the_published_reason()
-> Result<(), Box<dyn std::error::Error>> {
    let reads = Arc::new(AtomicUsize::new(0));
    let rules = SizeRules::default_bundle(source_of("size.agg.nope", reads.clone()))?;
    let error = rules
        .bind_latest()
        .err()
        .ok_or("an unknown ID was pinned")?;
    assert_eq!(error, EngineError::reason("space.rule_unknown"));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    Ok(())
}
