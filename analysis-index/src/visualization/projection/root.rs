//! The navigation-root level of a projection: rows, shapes, group and totals.
use super::{
    Detail, Error, Group, GroupMember, Lod, Model, Options, RemainderCauses, Row, Shape, Source,
    Totals, UnknownShare, gaps, group_ref, level,
};
use crate::visualization::{
    self, detail,
    filter::ItemFilter,
    geometry::{NoArea, valid_bounds},
    model_links::validate_model_links,
    ratio,
    typed_layout::{self, Dispatcher},
};
use du_core_contracts::{
    SizeAmount, SizeEvidence, SizeValue,
    result::{ResultItemKind, ResultVersionRead},
    visualization::model::ShapeMeta,
};
use std::num::NonZeroU64;

pub(super) fn build(
    source: Source<'_>,
    dispatcher: &Dispatcher,
    options: Options<'_>,
    filter: Option<ItemFilter>,
    cancelled: &mut dyn FnMut() -> bool,
    validate_links: bool,
) -> Result<Model, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let Source {
        version,
        parent,
        basis,
        bounds,
    } = source;
    if !valid_bounds(bounds) {
        return Err(Error::InvalidSource);
    }
    let min_area = detail::min_area(detail::rect_area(bounds), options.max_shapes);
    let filtered = filter.is_some();
    let level = level::build(
        level::Request {
            version,
            parent,
            basis: &basis,
            bounds,
            dispatcher,
            format: options.format,
            strategy: options.strategy,
            min_area,
            slots: options.max_shapes,
            allow_group: options.group_tail,
            filter,
        },
        cancelled,
    )?;
    let denominator = denominator(&level.parent_value)?;
    let is_folder = |item_no: u32| {
        version
            .items()
            .get(item_no as usize)
            .is_some_and(|item| item.kind == ResultItemKind::Directory)
    };
    let mut reasons = vec![None; level.rows.len()];
    for skipped in &level.skipped {
        reasons[skipped.row_index] = Some(skipped.reason);
    }
    let mut rows = Vec::with_capacity(level.rows.len() + level.unattributed.len());
    let mut omissions = Vec::with_capacity(level.skipped.len() + level.unattributed.len());
    for (index, row) in level.rows.iter().enumerate() {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let item = &row.child.item;
        let lod = if index < level.retained || index >= level.positive {
            Lod::Drawn
        } else if level.group.is_some() {
            Lod::Grouped(0)
        } else {
            Lod::Omitted
        };
        rows.push(Row {
            item: item.clone(),
            size: row.child.value.clone(),
            root_branch_ref: is_folder(item.item_no).then(|| item.clone()),
            area_value: visualization::area(&row.child.value),
            omission: reasons[index],
            omitted_children: version.children(item.item_no).len(),
            ratio: denominator.map(|total| ratio::format_ratio(i128::from(row.bytes), total)),
            lod,
        });
    }
    for skipped in &level.skipped {
        omissions.push(*skipped);
    }
    for unattributed in &level.unattributed {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let item = &unattributed.item;
        omissions.push(typed_layout::Omission {
            row_index: rows.len(),
            reason: NoArea::Unknown,
        });
        rows.push(Row {
            item: item.clone(),
            size: unattributed.value.clone(),
            root_branch_ref: is_folder(item.item_no).then(|| item.clone()),
            area_value: visualization::area(&unattributed.value),
            omission: Some(NoArea::Unknown),
            omitted_children: version.children(item.item_no).len(),
            ratio: denominator
                .zip(exact_bytes(&unattributed.value))
                .map(|(total, bytes)| ratio::format_ratio(i128::from(bytes), total)),
            lod: Lod::Drawn,
        });
    }
    let mut shapes = Vec::with_capacity(level.shapes.len());
    let mut drawn_bytes = 0_u64;
    let mut drawn_lower_bound = false;
    for (row_index, geometry) in &level.shapes {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let row = level.rows.get(*row_index).ok_or(Error::InvalidLayout)?;
        if row.bytes == 0 {
            return Err(Error::InvalidLayout);
        }
        drawn_bytes = drawn_bytes
            .checked_add(row.bytes)
            .ok_or(Error::InvalidLayout)?;
        drawn_lower_bound |= matches!(row.child.value.amount, SizeAmount::LowerBound { .. });
        let item = &row.child.item;
        shapes.push(Shape {
            row_index: *row_index,
            meta: ShapeMeta {
                shape_ref: format!("s{:x}-1", item.item_no),
                parent_shape_ref: None,
                visible_depth: 1,
                root_branch_ref: is_folder(item.item_no).then(|| item.clone()),
            },
            geometry: geometry.clone(),
        });
    }
    let omitted_items = level.omitted as u64
        + level
            .skipped
            .iter()
            .filter(|skipped| skipped.reason == NoArea::CoordinatePrecision)
            .count() as u64;
    let unknown_items = level.unattributed.len() as u64;
    let unknown_share = unknown_share(&level, filtered)?;
    let first_unknown_row = (!level.unattributed.is_empty()).then_some(level.rows.len());
    // The totals are defined on [Totals]: drawn adds the shaped root siblings above and
    // grouped adds this level's group below. No remainder is stored; it is the scope
    // minus these, and a difference of lower bounds is not a value. What it holds is
    // in the detail, from the two counts of this level.
    let remainder_causes = RemainderCauses {
        too_small: omitted_items > 0,
        unattributed: unknown_items > 0,
    };
    let mut totals = Totals {
        scope: level.parent_value.clone(),
        drawn_bytes,
        grouped_bytes: 0,
        filtered_bytes: level.filtered_bytes,
        drawn_lower_bound,
        grouped_lower_bound: false,
    };
    let mut groups = Vec::new();
    if let Some(group) = level.group {
        totals.grouped_bytes = group.bytes;
        totals.grouped_lower_bound = matches!(group.total.amount, SizeAmount::LowerBound { .. });
        groups.push(Group {
            parent: parent.clone(),
            group_ref: group_ref(parent),
            meta: ShapeMeta {
                shape_ref: format!("g{:x}-1", parent.item_no),
                parent_shape_ref: None,
                visible_depth: 1,
                root_branch_ref: None,
            },
            geometry: group.geometry,
            members: level.rows[level.retained..level.positive]
                .iter()
                .map(|row| GroupMember {
                    item: row.child.item.clone(),
                    size: row.child.value.clone(),
                })
                .collect(),
            total: group.total,
            size_range: group.size_range,
            slot: group.slot,
        });
    }
    let relationships = Vec::new();
    if validate_links {
        let metas: Vec<_> = shapes
            .iter()
            .map(|shape| shape.meta.clone())
            .chain(groups.iter().map(|group| group.meta.clone()))
            .collect();
        validate_model_links(version.version_id(), &metas, &relationships).map_err(Error::Links)?;
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(Model {
        result_revision: version.version_id().clone(),
        parent: parent.clone(),
        basis,
        completeness: version.completeness(),
        observed_at: version.observed_at().cloned(),
        strategy: level.descriptor,
        rows,
        shapes,
        descendants: Vec::new(),
        relationships,
        omissions,
        totals,
        filter,
        detail: Detail {
            min_area,
            group_tail: options.group_tail,
            groups,
            unknown_items,
            unknown_share,
            first_unknown_row,
            omitted_items,
            reason_keys: gaps::reasons(version, parent, cancelled)?,
            remainder_causes,
        },
    })
}

// The navigation-root share without an area: the parent's partition bytes less the
// attributed siblings' (see [UnknownShare]). It is exact unless the parent is a lower
// bound and an unread entry may lie under a sibling without an area, which F03 says by
// making that sibling's own value a lower bound.
fn unknown_share(level: &level::Level, filtered: bool) -> Result<UnknownShare, Error> {
    if level.unattributed.is_empty() {
        return Ok(UnknownShare::None);
    }
    if filtered {
        return Ok(UnknownShare::Unknown);
    }
    let bytes = level
        .parent_bytes
        .checked_sub(level.attributed_bytes)
        .ok_or(Error::InvalidLayout)?;
    let lower = matches!(level.parent_value.amount, SizeAmount::LowerBound { .. })
        && level
            .unattributed
            .iter()
            .any(|child| !matches!(child.value.amount, SizeAmount::Exact { .. }));
    Ok(match (bytes, lower) {
        (0, _) => UnknownShare::None,
        (bytes, true) => UnknownShare::LowerBound(bytes),
        (bytes, false) => UnknownShare::Exact(bytes),
    })
}

// The ratio denominator: an exact positive parent value; none for a lower bound.
fn denominator(parent_value: &SizeValue) -> Result<Option<NonZeroU64>, Error> {
    match &parent_value.amount {
        SizeAmount::Exact { bytes } => Ok(NonZeroU64::new(bytes.get())),
        SizeAmount::LowerBound { .. } => Ok(None),
        SizeAmount::Unknown => Err(Error::UnavailableAttribution),
    }
}

fn exact_bytes(value: &SizeValue) -> Option<u64> {
    match &value.amount {
        SizeAmount::Exact { bytes } if value.evidence != SizeEvidence::Unknown => Some(bytes.get()),
        _ => None,
    }
}
