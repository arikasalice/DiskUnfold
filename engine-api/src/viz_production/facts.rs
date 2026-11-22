//! Required-data facts read from one pinned result and folder.
use std::sync::Mutex;

use du_core_contracts::{Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue};

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
pub(super) fn row_size(
    version: &du_analysis_index::ResultVersion,
    item_no: u32,
    basis: &SizeBasis,
) -> Option<SizeValue> {
    version.item_space_size(item_no, basis.clone())
}

/// A size value that states an amount with evidence, as F03 reports it.
pub(super) fn has_value(size: Option<&SizeValue>) -> bool {
    size.is_some_and(|size| {
        size.amount != SizeAmount::Unknown && size.evidence != SizeEvidence::Unknown
    })
}

/// Remembers the facts of the folder read last.
///
/// One render request asks for the same folder facts twice, once for the available formats
/// and once for the required-data decision, and `viz.availability` asks for them again.
/// A pinned result never changes, so the facts are fixed by the result, the folder and the
/// size basis, and a repeated question is answered without reading the children again.
pub(super) struct FolderFacts {
    last: Mutex<Option<Remembered>>,
    /// Folder reads that were not answered from memory.
    #[cfg(test)]
    reads: std::sync::atomic::AtomicUsize,
}

struct Remembered {
    result: Id,
    scope: ItemRef,
    basis: SizeBasis,
    values: FactValues,
}

impl FolderFacts {
    pub(super) fn new() -> Self {
        Self {
            last: Mutex::new(None),
            #[cfg(test)]
            reads: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Reads the folder facts, or returns the remembered ones for the same question.
    ///
    /// A poisoned memory is skipped: the answer is read again and is the same.
    pub(super) fn read(
        &self,
        source: &CurrentAnalysis,
        scope: &ItemRef,
        basis: &SizeBasis,
    ) -> Result<FactValues, EngineError> {
        let result = source.result_version();
        if let Ok(last) = self.last.lock()
            && let Some(hit) = last
                .as_ref()
                .filter(|hit| &hit.result == result && &hit.scope == scope && &hit.basis == basis)
        {
            return Ok(hit.values.clone());
        }
        let values = scope_facts(source, scope, basis)?;
        #[cfg(test)]
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut last) = self.last.lock() {
            *last = Some(Remembered {
                result: result.clone(),
                scope: scope.clone(),
                basis: basis.clone(),
                values: values.clone(),
            });
        }
        Ok(values)
    }

    /// Folder reads so far that were not answered from memory.
    #[cfg(test)]
    pub(super) fn reads(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::SeqCst)
    }
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
    let basis_not_counted =
        sizes == Evidence::Absent && basis_not_counted(version, scope.item_no, basis);
    Ok(FactValues {
        folder_hierarchy: hierarchy,
        item_sizes: sizes,
        numeric_values: sizes,
        trend_observations: Evidence::Absent,
        comparison: Evidence::Absent,
        histogram_bins: Evidence::Known(()),
        attribution: Evidence::Absent,
        basis_not_counted,
    })
}

/// #149: the folder has no unique physical size only because sharing evidence
/// was not collected, while its allocated size is known. Any other basis, or a
/// folder without an allocated size (as on Linux), is not a basis without
/// evidence.
fn basis_not_counted(
    version: &du_analysis_index::ResultVersion,
    item_no: u32,
    basis: &SizeBasis,
) -> bool {
    let Some(space) = version.space(item_no) else {
        return false;
    };
    let known = |size: &SizeValue| size.amount != SizeAmount::Unknown;
    *basis == SizeBasis::UniquePhysical
        && !known(&space.unique_physical)
        && space.limitations.contains(&"space.sharing_not_collected")
        && known(&space.allocated)
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
            .any(|child| has_value(row_size(version, child.item_no, basis).as_ref()))
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
    values: FactValues,
    formats: &[String],
) -> Result<Vec<String>, EngineError> {
    let internal = || EngineError::reason("support.internal_error");
    let key = required_data::SourceKey::from_current(source, scope).map_err(|_| internal())?;
    let facts = required_data::Facts::bind(source, scope, values).map_err(|_| internal())?;
    let pinned = required_data::PinnedFacts::bind(source, scope, facts).map_err(|_| internal())?;
    let evaluated =
        required_data::format_availability(formats, &key, &pinned).map_err(|_| internal())?;
    Ok(evaluated.available.into_iter().map(str::to_owned).collect())
}
