use super::*;
use crate::aggregation_rule::RegisteredRule;
use crate::{IngestSink, ResultVersion};
use du_core_contracts::result::{ResultCompleteness, ResultNativePath, ResultScopeRoot};
use du_core_contracts::{Id, PathBytes, PathEncoding};
use std::sync::Arc;

const SUBTREE_V1: SizeRuleIdentity = SizeRuleIdentity {
    id: DEFAULT_SIZE_RULE_ID,
    revision: 1,
};
const SUBTREE_V2: SizeRuleIdentity = SizeRuleIdentity {
    id: DEFAULT_SIZE_RULE_ID,
    revision: 2,
};
const OTHER_V1: SizeRuleIdentity = SizeRuleIdentity {
    id: "size.agg.other",
    revision: 1,
};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

/// Registers the same implementation under a test-only identity.
fn test_rule(identity: SizeRuleIdentity) -> BoundSizeRule {
    BoundSizeRule::bind_as(identity, RegisteredRule::SubtreeUnique)
}

fn registry(identities: &[SizeRuleIdentity]) -> Result<SizeRuleRegistry, String> {
    let mut builder = SizeRuleRegistryBuilder::new();
    for identity in identities {
        builder
            .register(test_rule(*identity))
            .map_err(|e| format!("{e:?}"))?;
    }
    builder.build().map_err(|e| format!("{e:?}"))
}

fn root() -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    }
}

fn open(sink: &IngestSink, lineage: &Id, scope: u32) -> Result<BuildHandle, String> {
    sink.open_forest_in_lineage(id(scope)?, vec![root()], lineage.clone())
        .map_err(|e| format!("{e:?}"))
}

fn seal(mut build: BuildHandle, version: u32) -> Result<Arc<ResultVersion>, String> {
    build
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(version)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn duplicate_id_and_revision_is_rejected() {
    let mut builder = SizeRuleRegistryBuilder::new();
    assert_eq!(builder.register(test_rule(OTHER_V1)), Ok(()));
    assert_eq!(
        builder.register(test_rule(OTHER_V1)),
        Err(SizeRuleRegistryError::DuplicateRule)
    );
    assert_eq!(builder.register(test_rule(SUBTREE_V2)), Ok(()));
}

#[test]
fn default_bundle_cannot_be_registered_twice_and_leaves_one_entry() -> Result<(), String> {
    let mut builder = SizeRuleRegistryBuilder::new();
    assert_eq!(builder.register_defaults(), Ok(()));
    assert_eq!(
        builder.register_defaults(),
        Err(SizeRuleRegistryError::DuplicateRule)
    );
    let registry = builder.build().map_err(|e| format!("{e:?}"))?;
    assert_eq!(registry.identities().collect::<Vec<_>>(), [SUBTREE_V1]);
    assert_eq!(registry.default_rule().identity(), SUBTREE_V1);
    Ok(())
}

#[test]
fn build_requires_the_default_rule_id() {
    assert_eq!(
        SizeRuleRegistryBuilder::new().build().err(),
        Some(SizeRuleRegistryError::DefaultRuleMissing)
    );
    let mut builder = SizeRuleRegistryBuilder::new();
    assert_eq!(builder.register(test_rule(OTHER_V1)), Ok(()));
    assert_eq!(
        builder.build().err(),
        Some(SizeRuleRegistryError::DefaultRuleMissing)
    );
}

#[test]
fn unknown_id_or_revision_is_none() -> Result<(), String> {
    let registry = registry(&[SUBTREE_V1])?;
    assert!(registry.latest("size.agg.unknown").is_none());
    assert!(registry.resolve("size.agg.unknown", 1).is_none());
    assert!(registry.resolve(DEFAULT_SIZE_RULE_ID, 2).is_none());
    assert_eq!(
        registry
            .resolve(DEFAULT_SIZE_RULE_ID, 1)
            .map(|r| r.identity()),
        Some(SUBTREE_V1)
    );
    Ok(())
}

#[test]
fn latest_picks_the_highest_revision_of_the_id() -> Result<(), String> {
    let registry = registry(&[SUBTREE_V2, OTHER_V1, SUBTREE_V1])?;
    assert_eq!(
        registry.latest(DEFAULT_SIZE_RULE_ID).map(|r| r.identity()),
        Some(SUBTREE_V2)
    );
    assert_eq!(registry.default_rule().identity(), SUBTREE_V2);
    assert_eq!(
        registry.latest("size.agg.other").map(|r| r.identity()),
        Some(OTHER_V1)
    );
    assert_eq!(
        registry
            .resolve(DEFAULT_SIZE_RULE_ID, 1)
            .map(|r| r.identity()),
        Some(SUBTREE_V1)
    );
    Ok(())
}

#[test]
fn identities_are_listed_by_id_then_revision() -> Result<(), String> {
    let registry = registry(&[SUBTREE_V2, SUBTREE_V1, OTHER_V1])?;
    assert_eq!(
        registry.identities().collect::<Vec<_>>(),
        [OTHER_V1, SUBTREE_V1, SUBTREE_V2]
    );
    Ok(())
}

#[test]
fn bound_rule_identity_is_reported_and_replacement_keeps_it() -> Result<(), String> {
    let registry = registry(&[SUBTREE_V1, SUBTREE_V2])?;
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("size-rule-bound", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut build = open(&sink, &lineage, 2)?;
    build
        .set_size_rule(registry.resolve(DEFAULT_SIZE_RULE_ID, 2).ok_or("rev 2")?)
        .map_err(|e| format!("{e:?}"))?;
    let base = seal(build, 3)?;
    assert_eq!(base.size_rule(), SUBTREE_V2);

    let mut candidate = sink.open_replacement(&base).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        candidate.set_size_rule(registry.default_rule()),
        Err(BuildError::Unavailable)
    );
    candidate
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let next = candidate
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(next.size_rule(), SUBTREE_V2);
    Ok(())
}

#[test]
fn binding_is_refused_after_an_observation() -> Result<(), String> {
    let registry = registry(&[SUBTREE_V1])?;
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        build.set_size_rule(registry.default_rule()),
        Err(BuildError::Unavailable)
    );
    Ok(())
}
