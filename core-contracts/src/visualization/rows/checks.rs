//! Local invariants of one visualization row.
use super::VizRow;
use crate::{SizeAmount, SizeBasis};
use std::collections::HashSet;

impl VizRow {
    /// Checks local invariants. The caller must resolve group references in the pinned model.
    pub fn validate(&self) -> Result<(), &'static str> {
        self.validate_identity()?;
        self.validate_group_only_fields()?;
        self.validate_shared_allocation()?;
        if self.unread.is_some() && self.item_ref.is_none() {
            return Err("unread state on a group row");
        }
        self.validate_type_slices()?;
        self.validate_links()
    }

    fn validate_identity(&self) -> Result<(), &'static str> {
        if self.item_ref.is_some() == self.group_ref.is_some()
            || self.group_ref.as_ref().is_some_and(String::is_empty)
        {
            return Err("invalid row reference");
        }
        let mut seen = HashSet::new();
        if self
            .display_flags
            .iter()
            .any(|key| key.is_empty() || !seen.insert(key))
        {
            return Err("invalid display flags");
        }
        if self.label_key.as_ref().is_some_and(String::is_empty) {
            return Err("invalid label key");
        }
        Ok(())
    }

    fn validate_group_only_fields(&self) -> Result<(), &'static str> {
        // An item row shows a real file name. A message key there would let a
        // consumer replace that name with message text.
        if self.label_key.is_some() && (self.item_ref.is_some() || self.group_ref.is_none()) {
            return Err("label key on an item row");
        }
        if let Some(count) = &self.member_count {
            if self.item_ref.is_some() || self.group_ref.is_none() {
                return Err("member count on an item row");
            }
            if count.unit() != "items" {
                return Err("member count is not a count of items");
            }
        }
        if self.item_kind.is_some() && self.item_ref.is_none() {
            return Err("item kind on a group row");
        }
        if self.condition.is_some() && (self.item_ref.is_some() || self.group_ref.is_none()) {
            return Err("condition on an item row");
        }
        Ok(())
    }

    fn validate_shared_allocation(&self) -> Result<(), &'static str> {
        if (self.shares_space.is_some() || self.shared_counted_item.is_some())
            && self.size.basis != SizeBasis::Allocated
        {
            return Err("shared allocation fields on a non-allocated row");
        }
        if self.shares_space.is_some() && self.item_ref.is_none() {
            return Err("shared allocation field on a group row");
        }
        let Some(counted) = &self.shared_counted_item else {
            return Ok(());
        };
        let is_zero = match &self.size.amount {
            SizeAmount::Unknown => true,
            SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => bytes.get() == 0,
        };
        let own_item = self
            .item_ref
            .as_ref()
            .is_some_and(|item| item != counted && item.result_version == counted.result_version);
        if self.shares_space != Some(true) || !own_item || self.ratio.is_some() || !is_zero {
            return Err("invalid shared counted item");
        }
        Ok(())
    }
}
