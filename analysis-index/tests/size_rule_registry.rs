use du_analysis_index::size_rule_registry::{
    DEFAULT_SIZE_RULE_ID, SIZE_RULE_KIND, SizeRuleRegistryBuilder,
};
use du_analysis_index::{BuildError, IngestSink, SizeRuleIdentity};
use du_core_contracts::result::{ResultCompleteness, ResultNativePath, ResultScopeRoot};
use du_core_contracts::{Id, PathBytes, PathEncoding};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
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

fn error(e: BuildError) -> String {
    format!("{e:?}")
}

const SUBTREE_V1: SizeRuleIdentity = SizeRuleIdentity {
    id: "size.agg.subtree-unique",
    revision: 1,
};

#[test]
fn published_names_are_unchanged() {
    assert_eq!(SIZE_RULE_KIND, "size_rule");
    assert_eq!(DEFAULT_SIZE_RULE_ID, SUBTREE_V1.id);
}

#[test]
fn default_bundle_lists_and_resolves_subtree_unique() -> Result<(), String> {
    let mut builder = SizeRuleRegistryBuilder::new();
    builder.register_defaults().map_err(|e| format!("{e:?}"))?;
    let registry = builder.build().map_err(|e| format!("{e:?}"))?;
    assert_eq!(registry.identities().collect::<Vec<_>>(), [SUBTREE_V1]);
    let latest = registry.latest(DEFAULT_SIZE_RULE_ID).ok_or("latest")?;
    assert_eq!(latest.identity(), SUBTREE_V1);
    assert_eq!(registry.default_rule(), latest);
    assert!(registry.latest("size.agg.first-path").is_none());
    Ok(())
}

#[test]
fn bound_default_and_unbound_builds_report_the_same_identity() -> Result<(), String> {
    let mut builder = SizeRuleRegistryBuilder::new();
    builder.register_defaults().map_err(|e| format!("{e:?}"))?;
    let registry = builder.build().map_err(|e| format!("{e:?}"))?;
    let sink = IngestSink::default();

    let mut bound = sink.open_forest(id(1)?, vec![root()]).map_err(error)?;
    bound
        .set_size_rule(registry.default_rule())
        .map_err(error)?;
    bound.add_directory(Vec::new()).map_err(error)?;
    let bound = bound
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    assert_eq!(bound.size_rule(), SUBTREE_V1);

    let mut unbound = sink.open_forest(id(3)?, vec![root()]).map_err(error)?;
    unbound.add_directory(Vec::new()).map_err(error)?;
    let unbound = unbound
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(error)?;
    assert_eq!(unbound.size_rule(), SUBTREE_V1);
    Ok(())
}

#[test]
fn binding_is_refused_after_an_observation_or_a_partial_publication() -> Result<(), String> {
    let mut builder = SizeRuleRegistryBuilder::new();
    builder.register_defaults().map_err(|e| format!("{e:?}"))?;
    let rule = builder
        .build()
        .map_err(|e| format!("{e:?}"))?
        .default_rule();
    let sink = IngestSink::default();
    let mut build = sink.open_forest(id(1)?, vec![root()]).map_err(error)?;
    build.add_directory(Vec::new()).map_err(error)?;
    assert_eq!(build.set_size_rule(rule), Err(BuildError::Unavailable));
    build.publish_partial(id(2)?).map_err(error)?;
    assert_eq!(build.set_size_rule(rule), Err(BuildError::Unavailable));
    Ok(())
}
