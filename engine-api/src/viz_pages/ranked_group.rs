//! Atomic, source-verified publication of ranked bars and model-local groups.
mod grouping;
use super::{
    DeferredRows, Error, LayoutMetadata, ModelInput, ModelPages, PreparedModelInput, production,
};
use du_analysis_index::{
    ResultVersion,
    visualization::{
        bar_projection::{BarIdentity, Ranking},
        geometry::Weight,
        live_models::{GroupRows, PreparedRows, SizeRange},
        ratio,
        typed_layout::SelectedBar,
    },
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
    visualization::{LayoutDescriptor, geometry::Geometry, quantity::VizQuantity, rows::VizRow},
};
use grouping::{GroupRequest, make_group};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Weak},
};

/// Owner-provided model-local reference and display evidence for Other.
#[derive(Clone)]
pub struct OtherPresentation {
    /// Opaque reference scoped to this model.
    pub group_ref: String,
    /// Display name supplied by the presentation owner.
    pub display_name: String,
    /// Classification selected by the presentation owner.
    pub legend_class_key: String,
    /// Display flags supplied by their owner.
    pub display_flags: Vec<String>,
}

/// One selected bar and its corresponding row in the same fixed model.
pub struct RankedShape {
    /// Index of the corresponding row in this model.
    pub row_index: usize,
    /// Selected strategy geometry, absent when no area was assigned.
    pub geometry: Option<Geometry>,
    /// Reason for an absent area, when supplied by the strategy.
    pub no_area: Option<du_analysis_index::visualization::geometry::NoArea>,
}

/// Strategy and geometry retained with the fixed rows after publication.
pub struct RankedFrame {
    /// Exact descriptor selected from the session registry.
    pub descriptor: LayoutDescriptor,
    /// Ordered shapes and omissions indexed into this model's rows.
    pub shapes: Vec<RankedShape>,
}

/// An unpublished group transaction retained outside the F24 gate.
pub struct DeferredRankedGroup {
    owner: Weak<()>,
    input: PreparedModelInput,
    reference: String,
    frame: LayoutMetadata,
    cleanup: DeferredRows,
    used: bool,
}

/// A strategy result bound to the exact immutable source allocation it used.
pub struct PinnedRankedBar {
    pub(super) source: Arc<ResultVersion>,
    pub(super) selected: SelectedBar,
    pub(super) owner: Weak<()>,
}
impl PinnedRankedBar {
    /// Returns the registered result without permitting mutation of its source binding.
    pub fn selected(&self) -> &SelectedBar {
        &self.selected
    }
    pub(crate) fn into_selected(self) -> SelectedBar {
        self.selected
    }
}
impl DeferredRankedGroup {
    /// Returns the private reference reserved for the candidate.
    pub fn reserved_reference(&self) -> &str {
        &self.reference
    }
}

/// The pinned ranking, source presentation and model-local Other evidence.
pub struct Request<'a> {
    /// Session-produced strategy result and its immutable source allocation.
    pub pinned: PinnedRankedBar,
    /// Parent folder within the same result revision.
    pub scope: &'a ItemRef,
    /// F03 size basis fixed for this model.
    pub basis: SizeBasis,
    /// Classification and flags for every observed immediate child.
    pub presentations: Vec<production::RowPresentation>,
    /// Owner-provided Other reference and presentation.
    pub other: OtherPresentation,
    /// Caller-normalized condition fingerprint.
    pub fingerprint: String,
    /// Maximum number of bars admitted for this view.
    pub max_shapes: usize,
}

impl ModelPages {
    /// Verifies F03 membership and reserves a ranked group without publishing it.
    pub(super) fn defer_ranked_group(
        &self,
        request: Request<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<DeferredRankedGroup, Error> {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let Request {
            pinned,
            scope,
            basis,
            presentations,
            mut other,
            fingerprint,
            max_shapes,
        } = request;
        let PinnedRankedBar {
            source, selected, ..
        } = pinned;
        validate_request(&source, scope, &selected, &other, &fingerprint, max_shapes)?;
        other.display_flags.sort_unstable();
        other.display_flags.dedup();
        let mut by_item = original_rows(
            &source,
            scope,
            &basis,
            &selected,
            &fingerprint,
            presentations,
            cancelled,
        )?;
        let (rows, groups, shapes) = assemble(
            &source,
            &basis,
            selected.output,
            &mut by_item,
            other,
            cancelled,
        )?;
        if !by_item.is_empty() {
            return Err(Error::InvalidModel);
        }
        let key = candidate_key(
            &fingerprint,
            &selected.descriptor,
            &rows,
            &groups,
            &shapes,
            cancelled,
        )?;
        let input = ModelInput {
            version: source,
            condition_fingerprint: key,
            rows,
            groups,
        };
        super::validate_rows_cancellable(&input, &input.rows, cancelled)?;
        for group in input.groups.values() {
            super::validate_rows_cancellable(&input, &group.rows, cancelled)?;
        }
        let prepared = PreparedModelInput {
            version: input.version,
            condition_fingerprint: input.condition_fingerprint,
            rows: Some(
                PreparedRows::new_cancellable(input.rows, input.groups, cancelled)
                    .map_err(|_| Error::Cancelled)?,
            ),
            scope: Some(scope.clone()),
        };
        Ok(DeferredRankedGroup {
            owner: Arc::downgrade(&self.identity),
            input: prepared,
            reference: self.ids.next_id().as_str().to_owned(),
            frame: Arc::new(RankedFrame {
                descriptor: selected.descriptor,
                shapes,
            }),
            cleanup: DeferredRows::default(),
            used: false,
        })
    }

    /// Publishes reserved geometry, rows and groups under the caller's F24 gate.
    /// The candidate must remain alive until the caller releases that gate.
    pub(super) fn commit_ranked_group(
        &self,
        candidate: &mut DeferredRankedGroup,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<String, Error> {
        if candidate.used {
            return Err(Error::InvalidModel);
        }
        candidate.used = true;
        if !candidate
            .owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &self.identity))
        {
            return Err(Error::InvalidModel);
        }
        let (reference, _) = self.register_deferred(
            &mut candidate.input,
            &(None, Some(candidate.frame.clone())),
            &candidate.reference,
            cancelled,
            &mut candidate.cleanup,
        )?;
        Ok(reference)
    }

    /// Reads ranked strategy geometry associated with a current model reference.
    pub(super) fn ranked_frame(&self, reference: &str) -> Result<Arc<RankedFrame>, Error> {
        self.layout_metadata(reference)
    }
}

fn exact(size: &SizeValue) -> Option<u64> {
    if size.evidence == SizeEvidence::Unknown {
        return None;
    }
    match &size.amount {
        SizeAmount::Exact { bytes } => bytes.as_str().parse().ok(),
        _ => None,
    }
}

fn assemble(
    source: &ResultVersion,
    basis: &SizeBasis,
    ranking: Ranking,
    by_item: &mut HashMap<u32, VizRow>,
    other: OtherPresentation,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<RankedAssembly, Error> {
    let mut output = Vec::new();
    let mut groups = HashMap::new();
    let mut shapes = Vec::new();
    let mut seen = HashSet::new();
    let mut previous = None;
    for bar in ranking.bars {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if !groups.is_empty() {
            return Err(Error::InvalidModel);
        }
        let row = match bar.identity {
            BarIdentity::Item(item) => {
                if !seen.insert(item.item_no) || item.result_version != *source.version_id() {
                    return Err(Error::InvalidModel);
                }
                let row = by_item.remove(&item.item_no).ok_or(Error::InvalidModel)?;
                if row.item_ref.as_ref() != Some(&item) {
                    return Err(Error::InvalidModel);
                }
                row
            }
            BarIdentity::Other { members, size } => {
                let (row, group) = make_group(GroupRequest {
                    source,
                    basis,
                    members,
                    size,
                    by_item,
                    seen: &mut seen,
                    other: &other,
                    cancelled,
                })?;
                groups.insert(other.group_ref.clone(), group);
                row
            }
        };
        let value = exact(&row.size).ok_or(Error::InvalidModel)?;
        validate_shape(value, bar.value, bar.geometry.is_some(), bar.no_area)?;
        if let Some(item) = &row.item_ref {
            if previous
                .is_some_and(|(old, prior)| value > old || (value == old && item.item_no < prior))
            {
                return Err(Error::InvalidModel);
            }
            previous = Some((value, item.item_no));
        }
        shapes.push(RankedShape {
            row_index: output.len(),
            geometry: bar.geometry,
            no_area: bar.no_area,
        });
        output.push(row);
    }
    if groups.len() != 1 {
        return Err(Error::InvalidModel);
    }
    Ok((output, groups, shapes))
}

fn validate_request(
    source: &ResultVersion,
    scope: &ItemRef,
    selected: &SelectedBar,
    other: &OtherPresentation,
    fingerprint: &str,
    max_shapes: usize,
) -> Result<(), Error> {
    if fingerprint.is_empty()
        || other.group_ref.is_empty()
        || other.display_name.is_empty()
        || other.legend_class_key.is_empty()
        || other.display_flags.iter().any(String::is_empty)
        || selected.descriptor.format != "bar"
        || selected.output.result_version != *source.version_id()
        || scope.result_version != *source.version_id()
        || selected.output.bars.len() > max_shapes
        || selected.output.completeness != ResultCompleteness::Complete
        || selected.output.ranking_incomplete
        || !selected.output.unknown_items.is_empty()
    {
        return Err(Error::InvalidModel);
    }
    Ok(())
}

fn original_rows(
    source: &Arc<ResultVersion>,
    scope: &ItemRef,
    basis: &SizeBasis,
    selected: &SelectedBar,
    fingerprint: &str,
    presentations: Vec<production::RowPresentation>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<HashMap<u32, VizRow>, Error> {
    let original = production::project_fallback(
        source.clone(),
        scope,
        basis,
        fingerprint.to_owned(),
        presentations,
        cancelled,
    )
    .map_err(|error| {
        if error == production::Error::Cancelled {
            Error::Cancelled
        } else {
            Error::InvalidModel
        }
    })?;
    let scope_total = source.space(scope.item_no).ok_or(Error::InvalidModel)?;
    let scope_total = match basis {
        SizeBasis::Logical => &scope_total.logical,
        SizeBasis::Allocated => &scope_total.allocated,
        SizeBasis::UniquePhysical => &scope_total.unique_physical,
    };
    if selected.output.scope_total.result_version != *source.version_id()
        || selected.output.scope_total.target
            != du_analysis_index::visualization::bar_projection::AggregateTarget::Folder(
                scope.clone(),
            )
        || &selected.output.scope_total.size != scope_total
        || selected.output.denominator != selected.output.scope_total
    {
        return Err(Error::InvalidModel);
    }
    let mut sum = 0_u64;
    for row in &original.rows {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        sum = sum
            .checked_add(exact(&row.size).ok_or(Error::InvalidModel)?)
            .ok_or(Error::InvalidModel)?;
    }
    if Some(sum) != exact(scope_total) {
        return Err(Error::InvalidModel);
    }
    let mut by_item = HashMap::new();
    for row in original.rows {
        let item = row.item_ref.as_ref().ok_or(Error::InvalidModel)?;
        if by_item.insert(item.item_no, row).is_some() {
            return Err(Error::InvalidModel);
        }
    }
    Ok(by_item)
}

fn candidate_key(
    fingerprint: &str,
    descriptor: &LayoutDescriptor,
    rows: &[VizRow],
    groups: &HashMap<String, GroupRows>,
    shapes: &[RankedShape],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<String, Error> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"diskunfold.ranked-group.v1");
    serde_json::to_writer(
        &mut hash,
        &(
            &fingerprint,
            &descriptor.format,
            &descriptor.strategy.id,
            descriptor.strategy.revision,
            &rows,
        ),
    )
    .map_err(|_| Error::InvalidModel)?;
    for (key, group) in groups {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        serde_json::to_writer(&mut hash, &(key, &group.total, group.rows.len()))
            .map_err(|_| Error::InvalidModel)?;
        for row in &group.rows {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            serde_json::to_writer(&mut hash, row).map_err(|_| Error::InvalidModel)?;
        }
    }
    for shape in shapes {
        let omission = match shape.no_area {
            None => 0,
            Some(du_analysis_index::visualization::geometry::NoArea::Unknown) => 1,
            Some(du_analysis_index::visualization::geometry::NoArea::ExactZero) => 2,
            Some(du_analysis_index::visualization::geometry::NoArea::LowerBoundZero) => 3,
            Some(du_analysis_index::visualization::geometry::NoArea::CoordinatePrecision) => 4,
            Some(du_analysis_index::visualization::geometry::NoArea::UncertainOffset) => 5,
            Some(du_analysis_index::visualization::geometry::NoArea::UncertainScale) => 6,
        };
        serde_json::to_writer(&mut hash, &(shape.row_index, &shape.geometry, omission))
            .map_err(|_| Error::InvalidModel)?;
    }
    Ok(hash.finalize().to_hex().to_string())
}

type RankedAssembly = (Vec<VizRow>, HashMap<String, GroupRows>, Vec<RankedShape>);

fn validate_shape(
    value: u64,
    weight: Weight,
    has_geometry: bool,
    no_area: Option<du_analysis_index::visualization::geometry::NoArea>,
) -> Result<(), Error> {
    if weight != Weight::Exact(value)
        || (has_geometry && (no_area.is_some() || value == 0))
        || (!has_geometry && no_area.is_none())
        || (value > 0
            && matches!(
                no_area,
                Some(
                    du_analysis_index::visualization::geometry::NoArea::ExactZero
                        | du_analysis_index::visualization::geometry::NoArea::LowerBoundZero
                )
            ))
        || (value == 0
            && no_area != Some(du_analysis_index::visualization::geometry::NoArea::ExactZero))
    {
        return Err(Error::InvalidModel);
    }
    Ok(())
}
