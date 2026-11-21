//! Local checks of one model shape.

use super::{VizItemKind, VizMatchKey, VizShape};
use crate::Id;
use crate::visualization::type_slices::VizTypeSlice;
use std::collections::HashSet;

impl VizShape {
    pub(super) fn validate(&self, revision: &Id, treemap: bool) -> Result<(), &'static str> {
        let items = self
            .item_ref
            .iter()
            .chain(self.root_branch_ref.iter().flatten());
        if self.item_ref.is_some() == self.group_ref.is_some()
            || items
                .into_iter()
                .any(|item| item.result_version != *revision)
            || self.group_ref.as_ref().is_some_and(String::is_empty)
            || self.legend_class_key.is_empty()
            || matches!(&self.match_key, VizMatchKey::Unknown { reason_key } if reason_key.is_empty())
            || (self.group_ref.is_some()
                && !matches!(self.match_key, VizMatchKey::NotApplicable {}))
        {
            return Err("invalid shape reference or evidence");
        }
        let mut flags = HashSet::new();
        if self
            .display_flags
            .iter()
            .any(|flag| flag.is_empty() || !flags.insert(flag))
        {
            return Err("invalid shape flags");
        }
        if self
            .member_count
            .as_ref()
            .is_some_and(|count| self.group_ref.is_none() || count.unit() != "items")
        {
            return Err("member count on an item shape or not in items");
        }
        if self.item_kind.is_some() && self.item_ref.is_none() {
            return Err("item kind on a group shape");
        }
        self.validate_type_slices()?;
        if treemap
            && (self.shape_ref.is_none()
                || self.parent_shape_ref.is_none()
                || self.visible_depth.is_none()
                || self.root_branch_ref.is_none())
        {
            return Err("missing treemap hierarchy");
        }
        Ok(())
    }

    /// Slices belong to an item shape of a folder (F06 README 54).
    fn validate_type_slices(&self) -> Result<(), &'static str> {
        let Some(slices) = &self.type_slices else {
            return Ok(());
        };
        if self.item_ref.is_none() || self.item_kind != Some(VizItemKind::Folder) {
            return Err("type slices on a shape that is not a folder");
        }
        VizTypeSlice::validate_list(slices)
    }
}
