//! Validation and projection of exact ungrouped ranked bars.
use super::*;
use du_analysis_index::visualization::{
    bar_projection::{Bar, BarIdentity},
    geometry::NoArea,
    projection, ratio, typed_layout,
};
use du_core_contracts::{
    SizeAmount, SizeBasis, SizeValue,
    result::{ResultCompleteness, ResultItemKind},
    visualization::model::ShapeMeta,
};

pub(super) fn exact(size: &SizeValue) -> Option<u64> {
    if size.evidence == du_core_contracts::SizeEvidence::Unknown {
        return None;
    }
    match &size.amount {
        SizeAmount::Exact { bytes } => Some(bytes.get()),
        _ => None,
    }
}

/// Item bars with bytes that have no shape because they lack coordinate precision.
///
/// They are siblings left out of the drawn total, and the group of a bar is not one of
/// them: its members are counted by the group.
pub(super) fn precision_omitted(bars: &[Bar]) -> u64 {
    bars.iter()
        .filter(|bar| {
            matches!(bar.identity, BarIdentity::Item(_))
                && bar.no_area == Some(NoArea::CoordinatePrecision)
        })
        .count() as u64
}

pub(super) fn to_projection(
    admitted: &Admitted,
    selected: typed_layout::SelectedBar,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<projection::Model>, EngineError> {
    let ranking = selected.output;
    let source = admitted.source.version();
    let parent = &admitted.request.aggregate_scope_ref;
    if selected.descriptor != admitted.descriptor
        || ranking.result_version != *admitted.source.result_version()
        || ranking.denominator != ranking.scope_total
    {
        return Err(internal());
    }
    let Some(scope) = exact(&ranking.scope_total.size) else {
        return Ok(None);
    };
    if ranking.completeness != ResultCompleteness::Complete
        || ranking.ranking_incomplete
        || !ranking.unknown_items.is_empty()
    {
        return Ok(None);
    }
    let mut sum = 0_u64;
    let mut by_item = HashMap::new();
    let mut bytes_of = HashMap::new();
    for row in ranking.rows {
        if cancelled() {
            return Err(internal());
        }
        let Some(bytes) = exact(&row.size) else {
            return Ok(None);
        };
        sum = sum.checked_add(bytes).ok_or_else(internal)?;
        bytes_of.insert(row.item.item_no, bytes);
        if by_item.insert(row.item.item_no, row).is_some() {
            return Err(internal());
        }
    }
    // Every child is exact and the children sum to the exact scope total, so the
    // children partition the scope and a drawn subset adds up to its own bytes.
    if sum != scope {
        return Ok(None);
    }
    if ranking
        .bars
        .iter()
        .any(|bar| matches!(bar.identity, BarIdentity::Other { .. }))
    {
        return Err(internal());
    }
    let denominator =
        du_core_contracts::visualization::quantity::VizQuantity::exact("bytes", i128::from(scope))
            .ok_or_else(internal)?;
    let omitted = precision_omitted(&ranking.bars);
    let BarParts {
        rows,
        shapes,
        omissions,
        drawn,
    } = map_bars(source, ranking.bars, by_item, denominator, cancelled)?;
    let totals = source
        .selection_total(&drawn, true, cancelled)
        .map_err(|_| internal())?;
    let set_total = match &admitted.request.size_basis {
        SizeBasis::Logical => &totals.logical,
        SizeBasis::Allocated => &totals.allocated,
        SizeBasis::UniquePhysical => &totals.unique_physical,
    };
    // The F03 set total takes file rows only; a drawn folder leaves it without
    // independent membership, and the partition above gives the exact sum (#242).
    let drawn_bytes = match exact(set_total) {
        Some(bytes) => bytes,
        None => drawn_sum(&drawn, &bytes_of)?,
    };
    Ok(Some(projection::Model {
        result_revision: ranking.result_version,
        parent: parent.clone(),
        basis: admitted.request.size_basis.clone(),
        completeness: ranking.completeness,
        observed_at: source.observed_at().cloned(),
        strategy: selected.descriptor,
        rows,
        shapes,
        descendants: vec![],
        relationships: vec![],
        omissions,
        totals: projection::Totals {
            scope: ranking.scope_total.size,
            drawn_bytes,
            grouped_bytes: 0,
            filtered_bytes: 0,
            drawn_lower_bound: false,
            grouped_lower_bound: false,
        },
        detail: projection::Detail {
            omitted_items: omitted,
            remainder_causes: projection::RemainderCauses {
                too_small: omitted > 0,
                unattributed: false,
            },
            ..projection::Detail::default()
        },
    }))
}

struct BarParts {
    rows: Vec<projection::Row>,
    shapes: Vec<projection::Shape>,
    omissions: Vec<typed_layout::Omission>,
    drawn: Vec<du_core_contracts::ItemRef>,
}

/// The exact bytes of drawn children of a scope their exact values partition.
fn drawn_sum(
    drawn: &[du_core_contracts::ItemRef],
    bytes_of: &HashMap<u32, u64>,
) -> Result<u64, EngineError> {
    drawn.iter().try_fold(0_u64, |total, item| {
        let bytes = bytes_of.get(&item.item_no).ok_or_else(internal)?;
        total.checked_add(*bytes).ok_or_else(internal)
    })
}

fn map_bars(
    source: &du_analysis_index::ResultVersion,
    bars: Vec<du_analysis_index::visualization::bar_projection::Bar>,
    mut by_item: HashMap<u32, du_analysis_index::visualization::bar_projection::Row>,
    denominator: du_core_contracts::visualization::quantity::VizQuantity,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<BarParts, EngineError> {
    let mut rows = Vec::new();
    let mut shapes = Vec::new();
    let mut omissions = Vec::new();
    let mut drawn = Vec::new();
    let mut previous = None;
    for bar in bars {
        if cancelled() {
            return Err(internal());
        }
        let BarIdentity::Item(item) = bar.identity else {
            return Err(internal());
        };
        let row = by_item.remove(&item.item_no).ok_or_else(internal)?;
        if row.item != item {
            return Err(internal());
        }
        let value = exact(&row.size).ok_or_else(internal)?;
        if bar.value != du_analysis_index::visualization::geometry::Weight::Exact(value)
            || previous.is_some_and(|(prior_value, prior_item)| {
                value > prior_value || (value == prior_value && item.item_no < prior_item)
            })
        {
            return Err(internal());
        }
        previous = Some((value, item.item_no));
        if (bar.geometry.is_some() && (bar.no_area.is_some() || value == 0))
            || (bar.geometry.is_none() && bar.no_area.is_none())
        {
            return Err(internal());
        }
        let result_item = source.detail(&item).map_err(|_| internal())?.item;
        let branch = (result_item.kind == ResultItemKind::Directory).then(|| item.clone());
        let index = rows.len();
        rows.push(projection::Row {
            item: item.clone(),
            size: row.size,
            root_branch_ref: branch.clone(),
            area_value: if value == 0 {
                du_analysis_index::visualization::AreaValue::ExactZero
            } else {
                du_analysis_index::visualization::AreaValue::Positive
            },
            omission: bar.no_area,
            lod: projection::Lod::Drawn,
            omitted_children: if result_item.kind == ResultItemKind::Directory {
                du_analysis_index::browse::open(source, Some(&item))
                    .map_err(|_| internal())?
                    .children
                    .len()
            } else {
                0
            },
            ratio: ratio::calculate(
                &du_core_contracts::visualization::quantity::VizQuantity::exact(
                    "bytes",
                    i128::from(value),
                )
                .ok_or_else(internal)?,
                &denominator,
            ),
        });
        if let Some(geometry) = bar.geometry {
            drawn.push(item.clone());
            shapes.push(projection::Shape {
                row_index: index,
                meta: ShapeMeta {
                    shape_ref: format!("s{:x}-1", item.item_no),
                    parent_shape_ref: None,
                    visible_depth: 1,
                    root_branch_ref: branch,
                },
                geometry,
            });
        } else if let Some(reason) = bar.no_area {
            omissions.push(typed_layout::Omission {
                row_index: index,
                reason,
            });
        } else {
            return Err(internal());
        }
    }
    if !by_item.is_empty() {
        return Err(internal());
    }
    Ok(BarParts {
        rows,
        shapes,
        omissions,
        drawn,
    })
}
