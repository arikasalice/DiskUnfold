//! Required-data facts read from one pinned result and folder.
use du_core_contracts::{ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue};

use crate::{
    current_analysis::CurrentAnalysis,
    error::EngineError,
    required_data::{self, Evidence, FactValues},
};

/// Basis used where the request carries none.
///
/// `viz.availability` has no size basis; logical sizes are observed for every
/// item, so they answer the basis-independent question.
pub(super) const AVAILABILITY_BASIS: SizeBasis = SizeBasis::Logical;

/// Reads the F03 value an F06 row shows for `item_no` in `basis`.
///
/// This is the aggregate the row projection uses; no basis is converted.
pub(super) fn row_size<'a>(
    version: &'a du_analysis_index::ResultVersion,
    item_no: u32,
    basis: &SizeBasis,
) -> Option<&'a SizeValue> {
    let scope = version.space(item_no)?;
    Some(match basis {
        SizeBasis::Logical => &scope.logical,
        SizeBasis::Allocated => &scope.allocated,
        SizeBasis::UniquePhysical => &scope.unique_physical,
    })
}

/// A size value that states an amount with evidence, as F03 reports it.
pub(super) fn has_value(size: Option<&SizeValue>) -> bool {
    size.is_some_and(|size| {
        size.amount != SizeAmount::Unknown && size.evidence != SizeEvidence::Unknown
    })
}

/// Reads the facts of one pinned folder for the format table (HLD table 9).
///
/// Trend, comparison and attribution are facts about the source kind: a live scan
/// result is one observation and has no F27 trend, F08 comparison or
/// volume-folder-type attribution source attached. When such a source exists,
/// these three entries become reads of that source.
pub(super) fn scope_facts(
    source: &CurrentAnalysis,
    scope: &ItemRef,
    basis: &SizeBasis,
) -> Result<FactValues, EngineError> {
    let version = source.version();
    let (hierarchy, sizes) = match du_analysis_index::browse::open(version, Some(scope)) {
        Ok(folder) => (
            Evidence::Known(()),
            child_sizes(version, folder.children, basis),
        ),
        Err(_) => (Evidence::Unknown, Evidence::Unknown),
    };
    Ok(FactValues {
        folder_hierarchy: hierarchy,
        item_sizes: sizes,
        numeric_values: sizes,
        trend_observations: Evidence::Absent,
        comparison: Evidence::Absent,
        histogram_bins: Evidence::Known(()),
        attribution: Evidence::Absent,
    })
}

/// A folder without children has no size to lack; it is an empty model, not missing data.
fn child_sizes(
    version: &du_analysis_index::ResultVersion,
    children: &[du_core_contracts::result::ResultItem],
    basis: &SizeBasis,
) -> Evidence<()> {
    if children.is_empty()
        || children
            .iter()
            .any(|child| has_value(row_size(version, child.item_no, basis)))
    {
        Evidence::Known(())
    } else {
        Evidence::Absent
    }
}

/// Formats whose table row the pinned facts satisfy, in registry order.
pub(super) fn available_formats(
    source: &CurrentAnalysis,
    scope: &ItemRef,
    basis: &SizeBasis,
    formats: &[String],
) -> Result<Vec<String>, EngineError> {
    let internal = || EngineError::reason("support.internal_error");
    let values = scope_facts(source, scope, basis)?;
    let key = required_data::SourceKey::from_current(source, scope).map_err(|_| internal())?;
    let facts = required_data::Facts::bind(source, scope, values).map_err(|_| internal())?;
    let pinned = required_data::PinnedFacts::bind(source, scope, facts).map_err(|_| internal())?;
    let evaluated =
        required_data::format_availability(formats, &key, &pinned).map_err(|_| internal())?;
    Ok(evaluated.available.into_iter().map(str::to_owned).collect())
}
