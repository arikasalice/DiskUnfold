//! F06 format applicability table, kept separate from its evidence providers.
//!
//! Evidence is evaluated only after a caller pins its source. `Unknown` is not
//! absence: it must remain an internal unresolved result until the owning
//! provider can supply evidence. This module does not select or synthesize a
//! source, and its tests do not stand in for production providers.
#![allow(dead_code)] // Production evidence ports are not part of this change.

use du_core_contracts::Id;

/// Whether a pinned source proves a required fact, proves its absence, or cannot tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Evidence<T> {
    /// The pinned source proves the value.
    Known(T),
    /// The owner successfully read the pinned source and found no such value.
    Absent,
    /// The owning source or evidence provider is not available for this decision.
    Unknown,
}

/// Evidence needed by the F06 format table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FactValues {
    /// Whether folder hierarchy is established by the pinned result.
    pub(super) folder_hierarchy: Evidence<()>,
    /// Whether item size values are present in the pinned result.
    pub(super) item_sizes: Evidence<()>,
    /// Whether numeric item or category values are available to a bar format.
    pub(super) numeric_values: Evidence<()>,
    /// Number of real observation points in a pinned F27 trend result.
    pub(super) trend_observations: Evidence<usize>,
    /// Whether a pinned F08 comparison result is available.
    pub(super) comparison: Evidence<()>,
    /// Whether histogram unit boundaries are available.
    pub(super) histogram_bins: Evidence<()>,
    /// Attribution evidence for the pinned source and requested scope.
    pub(super) attribution: Evidence<Attribution>,
}

/// Evidence bundle labeled with the source revision inspected by its provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Facts {
    source: SourceKey,
    values: FactValues,
}

impl Facts {
    /// Associates one provider read with the current-analysis revision it inspected.
    pub fn bind(source: &crate::current_analysis::CurrentAnalysis, values: FactValues) -> Self {
        Self {
            source: SourceKey::from_current(source),
            values,
        }
    }
}

/// Identity of the one immutable source revision from which a facts bundle was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourceKey {
    lineage: Id,
    revision: Id,
}

impl SourceKey {
    /// Captures the identity from the already pinned current-analysis result.
    pub fn from_current(source: &crate::current_analysis::CurrentAnalysis) -> Self {
        Self {
            lineage: source.lineage().clone(),
            revision: source.result_version().clone(),
        }
    }

    #[cfg(test)]
    fn fixture(lineage: Id, revision: Id) -> Self {
        Self { lineage, revision }
    }
}

/// Evidence collected for exactly one immutable source revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PinnedFacts {
    source: SourceKey,
    facts: Facts,
}

impl PinnedFacts {
    /// Binds owner evidence to the immutable source identity used to obtain it.
    pub fn bind(source: &crate::current_analysis::CurrentAnalysis, facts: Facts) -> Self {
        Self {
            source: SourceKey::from_current(source),
            facts,
        }
    }

    #[cfg(test)]
    fn bind_fixture(source: SourceKey, facts: Facts) -> Self {
        Self { source, facts }
    }
}

/// Established properties of a pinned attribution summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Attribution {
    /// Whether any flow is negative; Sankey cannot represent negative flow width.
    pub has_negative_flow: bool,
}

/// Stable reasons from the published F06 unavailable response contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reason {
    /// Required baseline or comparison result is absent.
    NoBaseline,
    /// F27 has fewer than two real observation points.
    SingleObservation,
    /// Required spatial attribution summary is absent.
    NoAttribution,
    /// Required item size values or histogram boundaries are absent.
    NoItemSizes,
    /// A confirmed attribution summary contains negative flow.
    NegativeFlow,
}

impl Reason {
    /// Returns the published F06 reason key for this known missing-data condition.
    pub const fn key(self) -> &'static str {
        match self {
            Self::NoBaseline => "viz.unavailable.no_baseline",
            Self::SingleObservation => "viz.unavailable.single_observation",
            Self::NoAttribution => "viz.unavailable.no_attribution",
            Self::NoItemSizes => "viz.unavailable.no_item_sizes",
            Self::NegativeFlow => "viz.unavailable.negative_flow",
        }
    }
}

/// Result of evaluating one format against one pinned source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Evaluation {
    /// The pinned evidence satisfies every table row for this format.
    Available,
    /// A required value is known to be absent or incompatible.
    Unavailable(Reason),
}

/// Internal evaluation failures that must not be represented as a public absence reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    /// The caller evaluated facts against a different source revision.
    SourceMismatch,
    /// The source owner could not determine whether a requirement is present.
    EvidenceUnknown,
    /// The registry supplied a format that has no published F06 row.
    UnknownFormat,
}

#[derive(Clone, Copy)]
enum Requirement {
    FolderHierarchy,
    ItemSizes,
    NumericValues,
    TrendObservations,
    Comparison,
    HistogramBins,
    NonNegativeAttribution,
}

struct FormatRule {
    format: &'static str,
    strategy_id: &'static str,
    requirements: &'static [Requirement],
}

const FORMAT_RULES: &[FormatRule] = &[
    FormatRule {
        format: "treemap",
        strategy_id: "viz.treemap",
        requirements: &[Requirement::FolderHierarchy, Requirement::ItemSizes],
    },
    FormatRule {
        format: "bar",
        strategy_id: "viz.bar",
        requirements: &[Requirement::NumericValues],
    },
    FormatRule {
        format: "trend",
        strategy_id: "viz.trend",
        requirements: &[Requirement::TrendObservations],
    },
    FormatRule {
        format: "waterfall",
        strategy_id: "viz.waterfall",
        requirements: &[Requirement::Comparison],
    },
    FormatRule {
        format: "histogram",
        strategy_id: "viz.histogram",
        requirements: &[Requirement::ItemSizes, Requirement::HistogramBins],
    },
    FormatRule {
        format: "sankey",
        strategy_id: "viz.sankey",
        requirements: &[Requirement::NonNegativeAttribution],
    },
];

/// Evaluates the published requirement order for a format against one pinned source.
pub(super) fn evaluate(
    format: &str,
    source: &SourceKey,
    pinned: &PinnedFacts,
) -> Result<Evaluation, Error> {
    if &pinned.source != source || &pinned.facts.source != source {
        return Err(Error::SourceMismatch);
    }
    let rule = FORMAT_RULES
        .iter()
        .find(|rule| rule.format == format)
        .ok_or(Error::UnknownFormat)?;
    let _strategy_id = rule.strategy_id;
    for requirement in rule.requirements {
        match check(*requirement, &pinned.facts.values)? {
            Evaluation::Available => {}
            unavailable => return Ok(unavailable),
        }
    }
    Ok(Evaluation::Available)
}

/// Re-evaluates registered formats in registry order against the same pinned facts.
///
/// An unresolved decision prevents returning a partial list, which could imply that
/// a format is unavailable when its owner has not reported its evidence.
pub(super) fn available_formats<'a>(
    formats: &'a [String],
    source: &SourceKey,
    pinned: &PinnedFacts,
) -> Result<Vec<&'a str>, Error> {
    let mut available = Vec::with_capacity(formats.len());
    for format in formats {
        match evaluate(format, source, pinned)? {
            Evaluation::Available => available.push(format.as_str()),
            Evaluation::Unavailable(_) => {}
        }
    }
    Ok(available)
}

fn check(requirement: Requirement, facts: &FactValues) -> Result<Evaluation, Error> {
    Ok(match requirement {
        Requirement::FolderHierarchy => require(facts.folder_hierarchy, Reason::NoItemSizes)?,
        Requirement::ItemSizes => require(facts.item_sizes, Reason::NoItemSizes)?,
        Requirement::NumericValues => require(facts.numeric_values, Reason::NoItemSizes)?,
        Requirement::HistogramBins => require(facts.histogram_bins, Reason::NoItemSizes)?,
        Requirement::TrendObservations => match facts.trend_observations {
            Evidence::Known(count) if count >= 2 => Evaluation::Available,
            Evidence::Known(_) | Evidence::Absent => {
                Evaluation::Unavailable(Reason::SingleObservation)
            }
            Evidence::Unknown => return Err(Error::EvidenceUnknown),
        },
        Requirement::Comparison => require(facts.comparison, Reason::NoBaseline)?,
        Requirement::NonNegativeAttribution => match facts.attribution {
            Evidence::Known(Attribution {
                has_negative_flow: true,
            }) => Evaluation::Unavailable(Reason::NegativeFlow),
            Evidence::Known(Attribution {
                has_negative_flow: false,
            }) => Evaluation::Available,
            Evidence::Absent => Evaluation::Unavailable(Reason::NoAttribution),
            Evidence::Unknown => return Err(Error::EvidenceUnknown),
        },
    })
}

fn require(evidence: Evidence<()>, reason: Reason) -> Result<Evaluation, Error> {
    match evidence {
        Evidence::Known(()) => Ok(Evaluation::Available),
        Evidence::Absent => Ok(Evaluation::Unavailable(reason)),
        Evidence::Unknown => Err(Error::EvidenceUnknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMATS: [&str; 6] = [
        "treemap",
        "bar",
        "trend",
        "waterfall",
        "histogram",
        "sankey",
    ];

    /// One logical source fixture; this is not a production evidence provider.
    fn source_key(suffix: u8) -> SourceKey {
        let mut lineage = [0x00; 16];
        lineage[0] = 0x01;
        lineage[1] = 0x90;
        lineage[6] = 0x70;
        lineage[8] = 0x80;
        lineage[15] = suffix;
        let mut revision = lineage;
        revision[14] = suffix;
        SourceKey::fixture(Id::from_v7_bytes(lineage), Id::from_v7_bytes(revision))
    }

    /// One logical pinned-source fixture; it is not a production evidence provider.
    fn complete_fixture() -> FactValues {
        FactValues {
            folder_hierarchy: Evidence::Known(()),
            item_sizes: Evidence::Known(()),
            numeric_values: Evidence::Known(()),
            trend_observations: Evidence::Known(2),
            comparison: Evidence::Known(()),
            histogram_bins: Evidence::Known(()),
            attribution: Evidence::Known(Attribution {
                has_negative_flow: false,
            }),
        }
    }

    fn pinned(values: FactValues) -> (SourceKey, PinnedFacts) {
        let source = source_key(1);
        let facts = Facts {
            source: source.clone(),
            values,
        };
        let pinned = PinnedFacts::bind_fixture(source.clone(), facts);
        (source, pinned)
    }

    fn registry_formats() -> Vec<String> {
        FORMATS.iter().map(|format| (*format).to_owned()).collect()
    }

    #[test]
    fn every_published_format_is_available_when_its_pinned_facts_are_present() {
        let (source, pinned) = pinned(complete_fixture());

        for format in FORMATS {
            assert_eq!(
                evaluate(format, &source, &pinned),
                Ok(Evaluation::Available)
            );
        }
        assert_eq!(
            available_formats(&registry_formats(), &source, &pinned),
            Ok(FORMATS.to_vec())
        );
    }

    #[test]
    fn each_published_format_returns_its_first_missing_requirement_reason() {
        let cases = [
            (
                "treemap",
                Reason::NoItemSizes,
                FactValues {
                    folder_hierarchy: Evidence::Absent,
                    item_sizes: Evidence::Unknown,
                    ..complete_fixture()
                },
            ),
            (
                "bar",
                Reason::NoItemSizes,
                FactValues {
                    numeric_values: Evidence::Absent,
                    ..complete_fixture()
                },
            ),
            (
                "trend",
                Reason::SingleObservation,
                FactValues {
                    trend_observations: Evidence::Known(1),
                    ..complete_fixture()
                },
            ),
            (
                "waterfall",
                Reason::NoBaseline,
                FactValues {
                    comparison: Evidence::Absent,
                    ..complete_fixture()
                },
            ),
            (
                "histogram",
                Reason::NoItemSizes,
                FactValues {
                    item_sizes: Evidence::Absent,
                    histogram_bins: Evidence::Unknown,
                    ..complete_fixture()
                },
            ),
            (
                "sankey",
                Reason::NoAttribution,
                FactValues {
                    attribution: Evidence::Absent,
                    ..complete_fixture()
                },
            ),
        ];

        for (format, reason, facts) in cases {
            let (source, pinned) = pinned(facts);
            assert_eq!(
                evaluate(format, &source, &pinned),
                Ok(Evaluation::Unavailable(reason)),
                "{format}"
            );
            assert_eq!(reason.key(), expected_reason_key(reason));
        }
    }

    #[test]
    fn known_negative_sankey_flow_is_a_distinct_first_failure() {
        let facts = FactValues {
            attribution: Evidence::Known(Attribution {
                has_negative_flow: true,
            }),
            ..complete_fixture()
        };
        let (source, pinned) = pinned(facts);

        assert_eq!(
            evaluate("sankey", &source, &pinned),
            Ok(Evaluation::Unavailable(Reason::NegativeFlow))
        );
    }

    #[test]
    fn unknown_evidence_is_not_reported_as_an_unavailable_reason() {
        let facts = FactValues {
            folder_hierarchy: Evidence::Unknown,
            item_sizes: Evidence::Absent,
            ..complete_fixture()
        };
        let (source, pinned) = pinned(facts);

        assert_eq!(
            evaluate("treemap", &source, &pinned),
            Err(Error::EvidenceUnknown)
        );
        assert_eq!(
            available_formats(&registry_formats(), &source, &pinned),
            Err(Error::EvidenceUnknown)
        );
    }

    #[test]
    fn facts_from_a_different_result_revision_are_rejected() {
        let (_source, pinned) = pinned(complete_fixture());
        let other_source = source_key(2);

        assert_eq!(
            evaluate("bar", &other_source, &pinned),
            Err(Error::SourceMismatch)
        );
    }

    #[test]
    fn evidence_labeled_from_another_revision_is_rejected_during_binding() {
        let source = source_key(1);
        let facts = Facts {
            source: source_key(2),
            values: complete_fixture(),
        };
        let pinned = PinnedFacts::bind_fixture(source.clone(), facts);

        assert_eq!(
            evaluate("bar", &source, &pinned),
            Err(Error::SourceMismatch)
        );
    }

    #[test]
    fn unregistered_format_is_distinct_from_unresolved_evidence() {
        let (source, pinned) = pinned(complete_fixture());

        assert_eq!(
            evaluate("scatter", &source, &pinned),
            Err(Error::UnknownFormat)
        );
    }

    #[test]
    fn unavailable_format_list_is_recomputed_from_the_same_pinned_facts() {
        let facts = FactValues {
            attribution: Evidence::Absent,
            ..complete_fixture()
        };
        let (source, pinned) = pinned(facts);

        assert_eq!(
            available_formats(&registry_formats(), &source, &pinned),
            Ok(vec!["treemap", "bar", "trend", "waterfall", "histogram"])
        );
    }

    fn expected_reason_key(reason: Reason) -> &'static str {
        match reason {
            Reason::NoBaseline => "viz.unavailable.no_baseline",
            Reason::SingleObservation => "viz.unavailable.single_observation",
            Reason::NoAttribution => "viz.unavailable.no_attribution",
            Reason::NoItemSizes => "viz.unavailable.no_item_sizes",
            Reason::NegativeFlow => "viz.unavailable.negative_flow",
        }
    }
}
