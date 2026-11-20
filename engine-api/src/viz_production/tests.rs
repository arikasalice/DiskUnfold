//! Evidence read from published scan results; no fixture stands in for the F03 store.
use du_analysis_index::{IngestSink, Observation};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    size_units::{IEC_UNITS, SI_UNITS},
    visualization::{
        model::VizStalenessState, request::VizRenderRequest, rows::VizDenominatorBasis,
    },
};
use serde_json::json;
use std::sync::Arc;

use super::{ProductionEvidence, UnitTableSource, facts, groups, policy, presentation};
use crate::{
    api::Channel,
    current_analysis::{CurrentAnalysis, admit_scan},
    required_data::Evidence,
    viz_runtime::EvidenceProducer,
};

type TestResult = Result<(), String>;

mod basis;

fn id(value: u8) -> Id {
    Id::from_v7_bytes([value; 16])
}

fn size(bytes: u64) -> Option<SizeValue> {
    Some(SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    })
}

/// Publishes one directory scope holding the given files; `None` is a file without a size.
fn publish(files: &[(u8, Option<u64>)]) -> Result<CurrentAnalysis, String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|error| format!("{error:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut scan = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|error| format!("{error:?}"))?;
    scan.add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    for (name, bytes) in files {
        scan.add_file(Observation {
            path: vec![vec![*name]],
            size: bytes.and_then(size),
        })
        .map_err(|error| format!("{error:?}"))?;
    }
    scan.seal(id(3), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    admit_scan(&sink, None, None, Channel::Gui).map_err(|error| format!("{error:?}"))
}

fn scope() -> ItemRef {
    ItemRef {
        result_version: id(3),
        item_no: 0,
    }
}

fn request(color_criterion: &str, format: &str) -> Result<VizRenderRequest, String> {
    criteria_request(
        policy::CLASSIFICATION_CRITERION_NONE,
        color_criterion,
        format,
    )
}

fn criteria_request(
    classification_criterion: &str,
    color_criterion: &str,
    format: &str,
) -> Result<VizRenderRequest, String> {
    serde_json::from_value(json!({
        "viewChannel": id(9), "requestSeq": 1, "format": format, "resultRevision": id(3),
        "aggregateScopeRef": scope(), "navigationScopeRef": scope(),
        "measure": "size", "sizeBasis": "logical",
        "classificationCriterion": classification_criterion, "colorCriterion": color_criterion,
        "denominatorBasis": "scope", "viewport": {"width": 800, "height": 600, "scale": 1.0},
        "visibleWindow": {"left": 0, "top": 0, "right": 1_000_000, "bottom": 1_000_000},
        "minAreaPx2": 16, "depthLimit": 1,
    }))
    .map_err(|error| error.to_string())
}

fn producer(table: &'static du_core_contracts::size_units::UnitTable) -> ProductionEvidence {
    let unit_table: UnitTableSource = Arc::new(move || table);
    ProductionEvidence::new(vec!["treemap".into(), "trend".into()], 1, unit_table)
}

#[test]
fn size_state_maps_to_flags_only_when_proven() {
    let value = |evidence, amount| {
        Some(SizeValue {
            basis: SizeBasis::Logical,
            evidence,
            amount,
        })
    };
    let exact = SizeAmount::Exact { bytes: 5.into() };
    let lower = SizeAmount::LowerBound { bytes: 5.into() };
    assert!(
        presentation::size_flags(value(SizeEvidence::Measured, exact.clone()).as_ref()).is_empty()
    );
    assert_eq!(
        presentation::size_flags(value(SizeEvidence::OsReported, lower).as_ref()),
        [policy::FLAG_LOWER_BOUND]
    );
    for unproven in [
        value(SizeEvidence::Measured, SizeAmount::Unknown),
        value(SizeEvidence::Unknown, exact),
        None,
    ] {
        assert_eq!(
            presentation::size_flags(unproven.as_ref()),
            [policy::FLAG_UNKNOWN]
        );
    }
}

#[test]
fn path_and_match_keys_are_never_invented() -> TestResult {
    let source = publish(&[(b'a', Some(10)), (b'b', Some(20))])?;
    let request = request(policy::COLOR_CRITERION_NONE, "treemap")?;
    let producer = producer(&SI_UNITS);
    for item in source
        .version()
        .items()
        .iter()
        .filter(|item| item.parent == Some(0))
    {
        let evidence = producer
            .child(&source, item, &request)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(evidence.presentation.path_compare_key, None);
        assert_eq!(
            evidence.presentation.legend_class_key,
            policy::LEGEND_CLASS_UNCLASSIFIED
        );
        assert!(evidence.presentation.display_flags.is_empty());
        assert_eq!(
            evidence.match_key,
            du_core_contracts::visualization::model::VizMatchKey::Unknown {
                reason_key: policy::MATCH_KEY_REASON.into()
            }
        );
    }
    Ok(())
}

#[test]
fn scope_facts_read_sizes_and_keep_missing_sources_absent() -> TestResult {
    let source = publish(&[(b'a', Some(10)), (b'b', Some(20))])?;
    let facts = producer(&SI_UNITS)
        .required_data_facts_for_scope(&source, &scope())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(facts.folder_hierarchy, Evidence::Known(()));
    assert_eq!(facts.item_sizes, Evidence::Known(()));
    assert_eq!(facts.numeric_values, Evidence::Known(()));
    assert_eq!(facts.histogram_bins, Evidence::Known(()));
    assert_eq!(facts.trend_observations, Evidence::Absent);
    assert_eq!(facts.comparison, Evidence::Absent);
    assert_eq!(facts.attribution, Evidence::Absent);
    Ok(())
}

#[test]
fn children_without_any_size_value_lack_item_sizes() -> TestResult {
    let source = publish(&[(b'a', None), (b'b', None)])?;
    let facts = producer(&SI_UNITS)
        .required_data_facts_for_scope(&source, &scope())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(facts.folder_hierarchy, Evidence::Known(()));
    assert_eq!(facts.item_sizes, Evidence::Absent);
    Ok(())
}

#[test]
fn a_folder_without_children_is_an_empty_scope_not_missing_sizes() -> TestResult {
    let source = publish(&[])?;
    let facts = producer(&SI_UNITS)
        .required_data_facts_for_scope(&source, &scope())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(facts.item_sizes, Evidence::Known(()));
    Ok(())
}

#[test]
fn availability_and_render_agree_on_the_base_scope_facts() -> TestResult {
    let source = publish(&[(b'a', Some(10)), (b'b', None)])?;
    let producer = producer(&SI_UNITS);
    let scoped = producer
        .required_data_facts_for_scope(&source, &scope())
        .map_err(|error| format!("{error:?}"))?;
    let rendered = producer
        .required_data_facts(&source, &request(policy::COLOR_CRITERION_NONE, "treemap")?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(rendered, Some(scoped));
    Ok(())
}

#[test]
fn an_unlisted_color_criterion_is_rejected_as_measure_unknown() -> TestResult {
    let source = publish(&[(b'a', Some(10))])?;
    let producer = producer(&SI_UNITS);
    let rejected = producer.source_policy_facts(&source, &request("type", "treemap")?);
    assert_eq!(
        rejected.err(),
        Some(crate::error::EngineError::new(
            "viz.measure_unknown",
            vec![]
        ))
    );
    let accepted = producer
        .source_policy_facts(&source, &request(policy::COLOR_CRITERION_NONE, "treemap")?)
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing policy facts")?;
    assert_eq!(accepted.available_formats, ["treemap"]);
    assert_eq!(accepted.view_policy_revision, 1);
    assert!(
        accepted
            .classes
            .contains_key(policy::LEGEND_CLASS_UNCLASSIFIED)
    );
    Ok(())
}

#[test]
fn only_the_none_classification_criterion_is_served() -> TestResult {
    let source = publish(&[(b'a', Some(10))])?;
    let producer = producer(&SI_UNITS);
    for criterion in ["type", "", "None", " none", "fixture.criterion"] {
        let request = criteria_request(criterion, policy::COLOR_CRITERION_NONE, "treemap")?;
        assert_eq!(
            producer.source_policy_facts(&source, &request).err(),
            Some(crate::error::EngineError::new(
                "viz.measure_unknown",
                vec![]
            )),
            "{criterion:?}"
        );
    }
    let served = criteria_request(
        policy::CLASSIFICATION_CRITERION_NONE,
        policy::COLOR_CRITERION_NONE,
        "treemap",
    )?;
    assert!(
        producer
            .source_policy_facts(&source, &served)
            .map_err(|error| format!("{error:?}"))?
            .is_some()
    );
    Ok(())
}

#[test]
fn one_render_request_reads_its_folder_once() -> TestResult {
    let source = publish(&[(b'a', Some(10)), (b'b', Some(20))])?;
    let producer = producer(&SI_UNITS);
    let request = request(policy::COLOR_CRITERION_NONE, "treemap")?;
    producer
        .source_policy_facts(&source, &request)
        .map_err(|error| format!("{error:?}"))?;
    let rendered = producer
        .required_data_facts(&source, &request)
        .map_err(|error| format!("{error:?}"))?;
    // `viz.availability` asks about the same folder with the logical basis.
    let scoped = producer
        .required_data_facts_for_scope(&source, &scope())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(producer.folder_facts.reads(), 1);
    let direct = facts::scope_facts(&source, &scope(), &SizeBasis::Logical)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(rendered, Some(direct.clone()));
    assert_eq!(scoped, direct);
    Ok(())
}

#[test]
fn a_different_size_basis_is_read_again_and_not_answered_from_memory() -> TestResult {
    let source = publish(&[(b'a', Some(10)), (b'b', Some(20))])?;
    let producer = producer(&SI_UNITS);
    let logical = request(policy::COLOR_CRITERION_NONE, "treemap")?;
    let mut allocated = logical.clone();
    allocated.size_basis = SizeBasis::Allocated;
    let first = producer
        .required_data_facts(&source, &logical)
        .map_err(|error| format!("{error:?}"))?;
    let second = producer
        .required_data_facts(&source, &allocated)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(producer.folder_facts.reads(), 2);
    // The fixture observes logical sizes only, so the two bases give different facts.
    assert_ne!(first, second);
    let direct = facts::scope_facts(&source, &scope(), &SizeBasis::Allocated)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(second, Some(direct));
    Ok(())
}

#[test]
fn freshness_is_unknown_because_nothing_watches_the_result() -> TestResult {
    let source = publish(&[(b'a', Some(10))])?;
    let facts = producer(&SI_UNITS)
        .source_policy_facts(&source, &request(policy::COLOR_CRITERION_NONE, "treemap")?)
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing policy facts")?;
    // Being the current result of its lineage says nothing about the file system.
    assert_ne!(facts.staleness.state, VizStalenessState::Current);
    assert_eq!(facts.staleness.state, VizStalenessState::Unknown);
    assert_eq!(
        facts.staleness.reason_key.as_deref(),
        Some(policy::STALENESS_UNWATCHED_REASON)
    );
    Ok(())
}

#[test]
fn histogram_intervals_follow_the_selected_unit_table() -> TestResult {
    let source = publish(&[(b'a', Some(10))])?;
    for table in [&SI_UNITS, &IEC_UNITS] {
        let facts = producer(table)
            .histogram_facts(
                &source,
                &request(policy::COLOR_CRITERION_NONE, "histogram")?,
            )
            .map_err(|error| format!("{error:?}"))?
            .ok_or("missing histogram facts")?;
        let names: Vec<_> = facts
            .bins
            .iter()
            .map(|bin| bin.display_name.as_str())
            .collect();
        let (low, high) = if table.base() == 1000 {
            ("kB", "MB")
        } else {
            ("KiB", "MiB")
        };
        assert_eq!(names[0], format!("0 B\u{2013}1 {low}"));
        assert_eq!(names[1], format!("1 {low}\u{2013}1 {high}"));
        assert_eq!(names[5], format!("\u{2265} 1 {}", table.symbols()[5]));
        assert!(
            names
                .iter()
                .all(|name| !name.chars().any(|c| c.is_alphabetic() && !c.is_ascii()))
        );
        let refs: std::collections::HashSet<_> = facts
            .bins
            .iter()
            .map(|bin| bin.group_ref.as_str())
            .collect();
        assert_eq!(refs.len(), table.symbols().len());
        assert_eq!(facts.unknown_reason, policy::UNKNOWN_SIZE_REASON);
    }
    Ok(())
}

#[test]
fn the_other_group_names_a_message_key_for_treemap_and_bar_only() {
    for format in ["treemap", "bar"] {
        let other = groups::other_group(format);
        assert_eq!(
            other.map(|group| (group.display_name, group.label_key, group.display_flags)),
            Some((
                policy::GROUP_OTHER_NAME_KEY.to_owned(),
                Some(policy::GROUP_OTHER_NAME_KEY.to_owned()),
                vec![policy::FLAG_OTHER_GROUP.to_owned()]
            ))
        );
    }
    for format in ["histogram", "trend", "waterfall", "sankey"] {
        assert!(groups::other_group(format).is_none(), "{format}");
    }
}

#[test]
fn the_advertised_catalog_matches_what_render_accepts() -> TestResult {
    assert_eq!(
        serde_json::to_value(VizDenominatorBasis::Scope).map_err(|e| e.to_string())?,
        json!(policy::DENOMINATOR_SCOPE)
    );
    let catalog = policy::catalog_policy();
    assert_eq!(
        catalog.measures,
        [policy::MEASURE_SIZE, policy::MEASURE_COUNT]
    );
    assert_eq!(catalog.color_criteria, [policy::COLOR_CRITERION_NONE]);
    assert_eq!(catalog.denominator_criteria, [policy::DENOMINATOR_SCOPE]);
    Ok(())
}
