//! Checks of the model envelope.
use super::*;
use du_core_contracts::visualization::model::{VizMatchKey, VizShape};

fn shape(count: Option<VizQuantity>) -> Result<VizShape, EngineError> {
    Ok(VizShape {
        item_ref: None,
        group_ref: Some("g".into()),
        match_key: VizMatchKey::NotApplicable {},
        geometry: du_core_contracts::visualization::geometry::Geometry::new(
            "rect",
            vec![
                du_core_contracts::visualization::geometry::Point { x: 0, y: 0 },
                du_core_contracts::visualization::geometry::Point { x: 1, y: 1 },
            ],
        )
        .map_err(|_| internal())?,
        size: SizeValue {
            basis: du_core_contracts::SizeBasis::Logical,
            evidence: du_core_contracts::SizeEvidence::Measured,
            amount: SizeAmount::Exact {
                bytes: 1_u64.into(),
            },
        },
        quantity: None,
        legend_class_key: "class".into(),
        display_flags: vec![],
        shape_ref: None,
        parent_shape_ref: None,
        visible_depth: None,
        root_branch_ref: None,
        member_count: count,
        item_kind: None,
        type_slices: None,
        column: None,
    })
}

#[test]
fn the_grouped_total_adds_the_member_counts_and_keeps_what_is_not_exact() -> Result<(), EngineError>
{
    let exact = |n| VizQuantity::exact("items", n);
    let sum = |counts: Vec<Option<VizQuantity>>| -> Result<VizQuantity, EngineError> {
        let shapes = counts
            .into_iter()
            .map(shape)
            .collect::<Result<Vec<_>, _>>()?;
        grouped_items(&shapes)
    };
    // No group is zero items, and a shape without a count (an item) adds nothing.
    assert_eq!(sum(vec![])?, exact(0).ok_or_else(internal)?);
    assert_eq!(
        sum(vec![exact(40), exact(60), None])?,
        exact(100).ok_or_else(internal)?
    );
    // One count that is a lower bound makes the sum a lower bound.
    assert_eq!(
        sum(vec![exact(40), VizQuantity::lower_bound("items", 60)])?,
        VizQuantity::lower_bound("items", 100).ok_or_else(internal)?
    );
    // An unknown count makes the sum unknown, with its reason, and is not read as zero.
    let unknown = VizQuantity::unknown("items", "viz.unavailable.no_attribution".into());
    assert_eq!(
        sum(vec![exact(40), unknown.clone()])?,
        unknown.ok_or_else(internal)?
    );
    Ok(())
}

#[test]
fn a_table_only_fallback_publishes_the_no_attribution_key() {
    assert_eq!(
        super::fallback_reason(crate::viz_pages::render::FallbackReason::NoAttribution),
        crate::required_data::Reason::NoAttribution.key()
    );
    assert_eq!(
        super::fallback_reason(crate::viz_pages::render::FallbackReason::NoAttribution),
        "viz.unavailable.no_attribution"
    );
}
