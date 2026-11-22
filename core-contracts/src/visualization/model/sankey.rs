//! Checks of the Sankey fields of a model (#296).

use super::{VizModel, VizShape};
use crate::visualization::sankey::{SANKEY_LINK_KIND, VizColumn, VizLink};
use std::collections::HashMap;

impl VizModel {
    /// Columns and link amounts belong to a Sankey model only. There every shape
    /// has a column, and every link has exact bytes and goes one column right.
    pub(super) fn validate_sankey(
        &self,
        refs: &HashMap<&str, &VizShape>,
    ) -> Result<(), &'static str> {
        let sankey = self.representation.format == "sankey";
        if !sankey {
            let stray = self.shapes.iter().any(|shape| shape.column.is_some())
                || self
                    .relationships
                    .iter()
                    .any(|link| link.quantity.is_some());
            return if stray {
                Err("Sankey fields outside a Sankey model")
            } else {
                Ok(())
            };
        }
        if self.shapes.iter().any(|shape| shape.column.is_none()) {
            return Err("a Sankey shape without a column");
        }
        for link in self
            .relationships
            .iter()
            .filter(|link| link.kind == SANKEY_LINK_KIND)
        {
            if !link.quantity.as_ref().is_some_and(VizLink::exact_bytes) {
                return Err("a Sankey link without exact bytes");
            }
            let column = |key: &str| refs.get(key).and_then(|shape| shape.column);
            let (Some(from), Some(to)) = (column(&link.from_shape_ref), column(&link.to_shape_ref))
            else {
                return Err("a Sankey link between shapes without columns");
            };
            let unknown = from == VizColumn::Unknown || to == VizColumn::Unknown;
            if !unknown && !from.links_to(to) {
                return Err("a Sankey link against the column order");
            }
        }
        Ok(())
    }
}
