use super::*;
use crate::viz_pages::session::expansion::{ExpansionError, normalize_expansions};
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
        normalize(&mut request, &source, &descriptor.strategy)?;
        let facts = match self
            .assembly
            .producer
            .source_policy_facts(&source, &request)?
        {
            Some(policy) => owner_bound_facts(&source, policy),
            None => self.assembly.producer.source_facts(&source, &request)?,
        };
        validate_facts(&facts, &source, self.assembly.policy.revision)?;
        let required_data = required_data_admission(&self.assembly, &source, &request)?;
        let required_data_unavailable = matches!(
            required_data.as_ref().map(|value| value.evaluation),
            Some(crate::required_data::Evaluation::Unavailable(_))
        );
        let histogram = if request.format == "histogram" && !required_data_unavailable {
            Some(histogram_policy(
                &self.assembly,
                &source,
                &request,
                max_shapes,
                &facts.classes,
            )?)
        } else {
            None
        };
        let mut key = fingerprint(&request, &facts, max_shapes, required_data.as_ref())?;
        let ranked_other = if required_data_unavailable {
            None
        } else {
            bind_ranked_other(
                &*self.assembly.producer,
                &source,
                &request,
                &facts,
                &mut key,
            )?
        };
        if let Some(policy) = &histogram {
            let bins: Vec<_> = policy
                .bins
                .iter()
                .map(|bin| {
                    (
                        &bin.group_ref,
                        &bin.display_name,
                        &bin.legend_class_key,
                        &bin.display_flags,
                    )
                })
                .collect();
            let input = serde_json::to_vec(&(
                &key,
                policy.unit_table.base(),
                policy.unit_table.symbols(),
                policy.unit_table_revision,
                &policy.unknown_reason,
                bins,
            ))
            .map_err(|_| internal())?;
            key = blake3::hash(&input).to_hex().to_string();
        }
        Ok(Admitted {
            request,
            source,
            facts,
            key,
            descriptor,
            max_shapes,
            histogram,
            ranked_other,
            required_data,
        })
    }
    fn pin_source(&self, request: &VizRenderRequest) -> Result<CurrentAnalysis, EngineError> {
        if let Some(selection) = &self.selection {
            let selected = selection
                .selected_result_revision(&request.view_channel)
                .map_err(|_| internal())?
                .ok_or_else(unavailable)?;
            let source = crate::current_analysis::admit_selected(
                &self.assembly.source,
                &selected,
                request.result_revision.as_ref(),
            )
            .map_err(source_error)?;
            if let Some(reference) = &request.source_ref {
                if reference.kind != "scan" {
                    return Err(unavailable());
                }
                if reference.reference != *source.lineage() {
                    return Err(EngineError::reason("result.version_stale"));
                }
            }
            return Ok(source);
        }
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
        .map_err(source_error)
    }
}

fn histogram_policy(
    assembly: &Assembly,
    source: &CurrentAnalysis,
    request: &VizRenderRequest,
    max_shapes: usize,
    classes: &HashMap<String, LegendClass>,
) -> Result<HistogramFacts, EngineError> {
    let mut supplied = assembly
        .producer
        .histogram_facts(source, request)?
        .ok_or_else(unavailable)?;
    let mut group_refs = HashSet::new();
    if supplied.unit_table_revision == 0
        || supplied.unknown_reason.trim().is_empty()
        || supplied.bins.len() != supplied.unit_table.symbols().len()
        || supplied.bins.len() > max_shapes
        || supplied.bins.iter().any(|bin| {
            bin.group_ref.is_empty()
                || !group_refs.insert(&bin.group_ref)
                || bin.display_name.is_empty()
                || bin.legend_class_key.is_empty()
                || bin.display_flags.iter().any(String::is_empty)
                || !classes.contains_key(&bin.legend_class_key)
        })
    {
        return Err(invalid());
    }
    for bin in &mut supplied.bins {
        bin.display_flags.sort_unstable();
        bin.display_flags.dedup();
    }
    Ok(supplied)
}

fn required_data_admission(
    assembly: &Assembly,
    source: &CurrentAnalysis,
    request: &VizRenderRequest,
) -> Result<Option<RequiredDataAdmission>, EngineError> {
    let Some(values) = assembly.producer.required_data_facts(source, request)? else {
        return Ok(None);
    };
    let source_key =
        crate::required_data::SourceKey::from_current(source, &request.aggregate_scope_ref)
            .map_err(|_| internal())?;
    let facts = crate::required_data::Facts::bind(source, &request.aggregate_scope_ref, values)
        .map_err(|_| internal())?;
    let pinned =
        crate::required_data::PinnedFacts::bind(source, &request.aggregate_scope_ref, facts)
            .map_err(|_| internal())?;
    let evaluation = crate::required_data::evaluate(&request.format, &source_key, &pinned)
        .map_err(|_| internal())?;
    let formats = assembly.session.required_data_formats();
    let availability = crate::required_data::format_availability(&formats, &source_key, &pinned)
        .map_err(|_| internal())?;
    let available = availability
        .available
        .into_iter()
        .map(str::to_owned)
        .collect();
    let unresolved_formats = availability
        .unresolved
        .into_iter()
        .map(str::to_owned)
        .collect();
    Ok(Some(RequiredDataAdmission {
        formats: available,
        unresolved_formats,
        evaluation,
    }))
}

fn owner_bound_facts(source: &CurrentAnalysis, policy: SourcePolicyFacts) -> SourceFacts {
    let rule = source.version().size_rule();
    SourceFacts {
        staleness: policy.staleness,
        tables: VizTableRevisions {
            interpretation: policy.interpretation_revision,
            required_data: policy.required_data_revision,
            legend: policy.legend_revision,
            view_policy: policy.view_policy_revision,
            size_rule: du_core_contracts::browse_page_ipc::QueryStrategy {
                id: rule.id.into(),
                revision: rule.revision,
            },
        },
        classes: policy.classes,
        available_formats: policy.available_formats,
    }
}

fn source_error(error: crate::current_analysis::Error) -> EngineError {
    use crate::current_analysis::Error::*;
    EngineError::reason(match error {
        NoSource => "result.version_not_found",
        StaleVersion => "result.version_stale",
        UnsupportedSource => "ref.unavailable",
        Unavailable => "support.internal_error",
    })
}

fn validate_scope(request: &VizRenderRequest, source: &CurrentAnalysis) -> Result<(), EngineError> {
    request
        .validate_revision(source.result_version())
        .map_err(|_| invalid())?;
    for item in [&request.aggregate_scope_ref, &request.navigation_scope_ref]
        .into_iter()
        .chain(request.focus_item_ref.as_ref())
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
fn validate_facts(
    facts: &SourceFacts,
    source: &CurrentAnalysis,
    view_policy: u32,
) -> Result<(), EngineError> {
    // The scan build binds the rule before publication. Model evidence must
    // describe that same rule, including when the configured default changes.
    let applied = source.version().size_rule();
    if facts.tables.view_policy != view_policy
        || facts.tables.size_rule.id != applied.id
        || facts.tables.size_rule.revision != applied.revision
        || [
            facts.tables.interpretation,
            facts.tables.required_data,
            facts.tables.legend,
        ]
        .contains(&0)
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
) -> Result<(), EngineError> {
    request.source_ref = Some(du_core_contracts::visualization::request::VizSourceRef {
        kind: "scan".into(),
        reference: source.lineage().clone(),
    });
    request.result_revision = Some(source.result_version().clone());
    request.strategy = Some(du_core_contracts::browse_page_ipc::QueryStrategy {
        id: strategy.id.clone(),
        revision: strategy.revision,
    });
    let entries = request.expanded_depth_by_item.take();
    let expansions = normalize_expansions(
        source,
        &request.aggregate_scope_ref,
        request.depth_limit,
        entries.as_deref().unwrap_or_default(),
        &mut || false,
    )
    .map_err(|error| match error {
        ExpansionError::Limit => EngineError::reason("viz.viewport_invalid"),
        ExpansionError::Invalid => invalid(),
        ExpansionError::Cancelled => internal(),
    })?;
    if request.format == "treemap" {
        request.expanded_depth_by_item = (!expansions.is_empty()).then_some(expansions);
        request.color_mode = Some(request.color_mode.unwrap_or(VizColorMode::ParentFolder));
        request.label_mode = Some(request.label_mode.unwrap_or(VizLabelMode::NameSize));
    } else {
        request.color_mode = None;
        request.label_mode = None;
    }
    Ok(())
}
fn fingerprint(
    request: &VizRenderRequest,
    facts: &SourceFacts,
    max_shapes: usize,
    required_data: Option<&RequiredDataAdmission>,
) -> Result<String, EngineError> {
    let mut value = serde_json::to_value(request).map_err(|_| internal())?;
    let normalized = value.as_object_mut().ok_or_else(internal)?;
    normalized.remove("viewChannel");
    normalized.remove("requestSeq");
    let bytes = serde_json::to_vec(&(
        value,
        &facts.tables,
        &facts.staleness,
        max_shapes,
        required_data,
    ))
    .map_err(|_| internal())?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn bind_ranked_other(
    producer: &dyn EvidenceProducer,
    source: &CurrentAnalysis,
    request: &VizRenderRequest,
    facts: &SourceFacts,
    key: &mut String,
) -> Result<Option<crate::viz_pages::ranked_group::OtherPresentation>, EngineError> {
    let mut ranked_other = if request.format == "bar" {
        producer.ranked_other(source, request)?
    } else {
        None
    };
    if let Some(other) = &mut ranked_other {
        if other.group_ref.is_empty()
            || other.display_name.is_empty()
            || !facts.classes.contains_key(&other.legend_class_key)
            || other.display_flags.iter().any(String::is_empty)
        {
            return Err(invalid());
        }
        other.display_flags.sort_unstable();
        other.display_flags.dedup();
        let input = serde_json::to_vec(&(
            &*key,
            &other.group_ref,
            &other.display_name,
            &other.legend_class_key,
            &other.display_flags,
        ))
        .map_err(|_| internal())?;
        *key = blake3::hash(&input).to_hex().to_string();
    }
    Ok(ranked_other)
}
