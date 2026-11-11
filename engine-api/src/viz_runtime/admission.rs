use super::*;
use du_core_contracts::visualization::{
    LayoutStrategyKey,
    geometry::COORDINATE_MAX,
    render::{VizColorMode, VizLabelMode},
    rows::VizDenominatorBasis,
};
impl Runtime {
    pub(super) fn admit(&self, mut request: VizRenderRequest) -> Result<Admitted, EngineError> {
        self.regions
            .validate_region(&self.owner, &request.view_channel, ())
            .map_err(|_| unavailable())?;
        let source = self.pin_source(&request)?;
        validate_scope(&request, &source)?;
        let max_shapes = validate_viewport(&request, &self.assembly.policy)?;
        let strategy = request.strategy.as_ref().map(|s| LayoutStrategyKey {
            id: s.id.clone(),
            revision: s.revision,
        });
        let descriptor = self
            .assembly
            .session
            .select_strategy(&request.format, strategy.as_ref())
            .map_err(|error| {
                EngineError::reason(match error {
                    crate::viz_catalog::Error::UnknownFormat => "viz.format_unknown",
                    crate::viz_catalog::Error::UnknownStrategy => "viz.strategy_unknown",
                    _ => "support.internal_error",
                })
            })?;
        normalize(&mut request, &source, &descriptor.strategy);
        let facts = self.assembly.producer.source_facts(&source, &request)?;
        validate_facts(&facts, self.assembly.policy.revision)?;
        let key = fingerprint(&request, &facts, max_shapes)?;
        Ok(Admitted {
            request,
            source,
            facts,
            key,
            strategy: descriptor.strategy,
            max_shapes,
        })
    }
    fn pin_source(&self, request: &VizRenderRequest) -> Result<CurrentAnalysis, EngineError> {
        let lineage = match &request.source_ref {
            Some(reference) if reference.kind == "scan" => Some(&reference.reference),
            Some(_) => return Err(unavailable()),
            None => None,
        };
        crate::current_analysis::admit_scan(
            &self.assembly.source,
            lineage,
            request.result_revision.as_ref(),
        )
        .map_err(|error| {
            use crate::current_analysis::Error::*;
            EngineError::reason(match error {
                NoSource => "result.version_not_found",
                StaleVersion => "result.version_stale",
                UnsupportedSource => "ref.unavailable",
                Unavailable => "support.internal_error",
            })
        })
    }
}
fn validate_scope(request: &VizRenderRequest, source: &CurrentAnalysis) -> Result<(), EngineError> {
    request
        .validate_revision(source.result_version())
        .map_err(|_| invalid())?;
    for item in [&request.aggregate_scope_ref, &request.navigation_scope_ref]
        .into_iter()
        .chain(request.focus_item_ref.as_ref())
        .chain(
            request
                .expanded_depth_by_item
                .iter()
                .flatten()
                .map(|x| &x.item_ref),
        )
    {
        source.version().detail(item).map_err(|_| unavailable())?;
    }
    du_analysis_index::browse::open(source.version(), Some(&request.aggregate_scope_ref))
        .map_err(|_| EngineError::reason("browse.not_folder"))?;
    if request.measure != "size" {
        return Err(EngineError::reason("viz.measure_unknown"));
    }
    if request.denominator_basis != VizDenominatorBasis::Scope {
        return Err(EngineError::reason("viz.denominator_invalid"));
    }
    // The currently registered implementation supports one unfiltered parent.
    if request.condition.is_some()
        || request.aggregate_scope_ref != request.navigation_scope_ref
        || request
            .focus_item_ref
            .as_ref()
            .is_some_and(|x| x != &request.aggregate_scope_ref)
        || (request.depth_limit != 1 && !(request.format == "treemap" && request.depth_limit == 2))
        || request.format == "treemap"
            && request
                .expanded_depth_by_item
                .as_ref()
                .is_some_and(|entries| entries.iter().any(|x| x.extra_depth != 0))
    {
        return Err(invalid());
    }
    Ok(())
}
fn validate_viewport(
    request: &VizRenderRequest,
    policy: &ViewPolicy,
) -> Result<usize, EngineError> {
    let invalid = || EngineError::reason("viz.viewport_invalid");
    if !request.viewport.scale.is_finite()
        || request.viewport.scale <= 0.0
        || request.viewport.scale > policy.max_scale
        || !(4..=256).contains(&request.min_area_px2)
        || request.visible_window.left >= request.visible_window.right
        || request.visible_window.top >= request.visible_window.bottom
        || request.visible_window.right > COORDINATE_MAX
        || request.visible_window.bottom > COORDINATE_MAX
    {
        return Err(invalid());
    }
    let area = u64::from(request.viewport.width.get()) * u64::from(request.viewport.height.get());
    let max_shapes = usize::try_from(area / u64::from(request.min_area_px2))
        .map_err(|_| invalid())?
        .min(policy.max_shapes);
    if max_shapes == 0 {
        return Err(invalid());
    }
    Ok(max_shapes)
}
fn validate_facts(facts: &SourceFacts, view_policy: u32) -> Result<(), EngineError> {
    if facts.tables.view_policy != view_policy
        || [
            facts.tables.interpretation,
            facts.tables.required_data,
            facts.tables.legend,
            facts.tables.size_rule.revision,
        ]
        .contains(&0)
        || facts.tables.size_rule.id.is_empty()
        || facts
            .classes
            .iter()
            .any(|(key, value)| key.is_empty() || value.label_key.is_empty())
    {
        return Err(internal());
    }
    Ok(())
}
fn normalize(
    request: &mut VizRenderRequest,
    source: &CurrentAnalysis,
    strategy: &LayoutStrategyKey,
) {
    request.source_ref = Some(du_core_contracts::visualization::request::VizSourceRef {
        kind: "scan".into(),
        reference: source.lineage().clone(),
    });
    request.result_revision = Some(source.result_version().clone());
    request.strategy = Some(du_core_contracts::browse_page_ipc::QueryStrategy {
        id: strategy.id.clone(),
        revision: strategy.revision,
    });
    request.expanded_depth_by_item = None;
    if request.format == "treemap" {
        request.color_mode = Some(request.color_mode.unwrap_or(VizColorMode::ParentFolder));
        request.label_mode = Some(request.label_mode.unwrap_or(VizLabelMode::NameSize));
    } else {
        request.color_mode = None;
        request.label_mode = None;
    }
}
fn fingerprint(
    request: &VizRenderRequest,
    facts: &SourceFacts,
    max_shapes: usize,
) -> Result<String, EngineError> {
    let mut value = serde_json::to_value(request).map_err(|_| internal())?;
    let normalized = value.as_object_mut().ok_or_else(internal)?;
    normalized.remove("viewChannel");
    normalized.remove("requestSeq");
    let bytes = serde_json::to_vec(&(value, &facts.tables, &facts.staleness, max_shapes))
        .map_err(|_| internal())?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}
