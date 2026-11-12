//! F06 format applicability table, kept separate from its evidence providers.
//!
//! Evidence is evaluated only after a caller pins its source. `Unknown` is not
//! absence: it must remain an internal unresolved result until the owning
//! provider can supply evidence. This module does not select or synthesize a
//! source, and its tests do not stand in for production providers.
use du_core_contracts::Id;

/// Whether a pinned source proves a required fact, proves its absence, or cannot tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence<T> {
    /// The pinned source proves the value.
    Known(T),
    /// The owner successfully read the pinned source and found no such value.
    Absent,
    /// The owning source or evidence provider is not available for this decision.
    Unknown,
}

/// Evidence needed by the F06 format table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactValues {
    /// Whether folder hierarchy is established by the pinned result.
    pub folder_hierarchy: Evidence<()>,
    /// Whether item size values are present in the pinned result.
    pub item_sizes: Evidence<()>,
    /// Whether numeric item or category values are available to a bar format.
    pub numeric_values: Evidence<()>,
    /// Number of real observation points in a pinned F27 trend result.
    pub trend_observations: Evidence<usize>,
    /// Whether a pinned F08 comparison result is available.
    pub comparison: Evidence<()>,
    /// Whether histogram unit boundaries are available.
    pub histogram_bins: Evidence<()>,
    /// Attribution evidence for the pinned source and requested scope.
    pub attribution: Evidence<Attribution>,
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
pub struct Attribution {
    /// Whether any flow is negative; Sankey cannot represent negative flow width.
    pub has_negative_flow: bool,
}

/// Stable reasons from the published F06 unavailable response contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
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
    requirements: &'static [Requirement],
}

const FORMAT_RULES: &[FormatRule] = &[
    FormatRule {
        format: "treemap",
        requirements: &[Requirement::FolderHierarchy, Requirement::ItemSizes],
    },
    FormatRule {
        format: "bar",
        requirements: &[Requirement::NumericValues],
    },
    FormatRule {
        format: "trend",
        requirements: &[Requirement::TrendObservations],
    },
    FormatRule {
        format: "waterfall",
        requirements: &[Requirement::Comparison],
    },
    FormatRule {
        format: "histogram",
        requirements: &[Requirement::ItemSizes, Requirement::HistogramBins],
    },
    FormatRule {
        format: "sankey",
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
    for requirement in rule.requirements {
        match check(*requirement, &pinned.facts.values)? {
            Evaluation::Available => {}
            unavailable => return Ok(unavailable),
        }
    }
    Ok(Evaluation::Available)
}

/// Re-evaluates registered formats in registry order against the same pinned facts.
/// Unknown formats are retained as unresolved and omitted from the positive list.
/// Positively available and unresolved registered formats, evaluated on one pinned source.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct FormatAvailability<'a> {
    pub available: Vec<&'a str>,
    pub unresolved: Vec<&'a str>,
}

/// Omits unresolved formats from the public positive list while retaining them internally.
pub(super) fn format_availability<'a>(
    formats: &'a [String],
    source: &SourceKey,
    pinned: &PinnedFacts,
) -> Result<FormatAvailability<'a>, Error> {
    let mut availability = FormatAvailability {
        available: Vec::with_capacity(formats.len()),
        unresolved: Vec::new(),
    };
    for format in formats {
        match evaluate(format, source, pinned) {
            Ok(Evaluation::Available) => availability.available.push(format),
            Ok(Evaluation::Unavailable(_)) => {}
            Err(Error::EvidenceUnknown) => availability.unresolved.push(format),
            Err(error) => return Err(error),
        }
    }
    Ok(availability)
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
mod tests;
