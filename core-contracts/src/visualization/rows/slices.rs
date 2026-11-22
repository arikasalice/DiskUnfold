//! Local checks of a row's type slices (F06 README 54) and Sankey links (#296).

use super::VizRow;
use crate::visualization::item_kind::VizItemKind;
use crate::visualization::sankey::VizLink;
use crate::visualization::type_slices::{OTHER_GROUP_LABEL_KEY, VizTypeSlice};

impl VizRow {
    /// A Sankey node row's links: exact amounts to distinct rows.
    pub(super) fn validate_links(&self) -> Result<(), &'static str> {
        self.links.as_deref().map_or(Ok(()), VizLink::validate_list)
    }

    /// Slices belong to a folder item row or to the Other group row.
    pub(super) fn validate_type_slices(&self) -> Result<(), &'static str> {
        let Some(slices) = &self.type_slices else {
            return Ok(());
        };
        if self.item_ref.is_some() && self.item_kind != Some(VizItemKind::Folder) {
            return Err("type slices on an item row that is not a folder");
        }
        if self.item_ref.is_none() && self.label_key.as_deref() != Some(OTHER_GROUP_LABEL_KEY) {
            return Err("type slices on a group row that is not the Other group");
        }
        VizTypeSlice::validate_list(slices)
    }
}
