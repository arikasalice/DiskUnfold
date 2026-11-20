//! Why an observed attribute of an item is unknown (F02 FR2.HD1, stage 2 in F03 #239).
//!
//! Only an attribute the observation could not establish is kept, never a known or an
//! inapplicable one: one sparse row of (item, attribute) with a small reason number that
//! maps one-to-one to an [`attribute_reason`] constant. No string is stored per item.
//! A publication holds five bytes per row, at most four rows per item; the build holds
//! eight bytes per recorded row until it ends.
use crate::{BuildError, BuildHandle, ResultVersion};
use du_core_contracts::scan_observation::attribute_reason;

/// An optional attribute of a metadata observation whose unknown reason is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObservedAttribute {
    ModifiedAt,
    Allocated,
    LinkCount,
    SpaceIdentity,
}

/// The registered reasons; reason number `n` is `REASONS[n - 1]`.
const REASONS: [&str; 4] = [
    attribute_reason::QUERY_FAILED,
    attribute_reason::UNSUPPORTED,
    attribute_reason::OTHER_VOLUME,
    attribute_reason::PLACEHOLDER,
];

/// The attribute in the high four bits and the reason number in the low four.
fn code(attribute: ObservedAttribute, reason: &str) -> Option<u8> {
    let at = REASONS.iter().position(|known| *known == reason)?;
    let number = u8::try_from(at + 1).ok()?;
    Some((attribute as u8) << 4 | number)
}

fn attribute_of(code: u8) -> u8 {
    code >> 4
}

fn reason_of(code: u8) -> Option<&'static str> {
    let number = usize::from(code & 0x0f);
    REASONS.get(number.checked_sub(1)?).copied()
}

impl BuildHandle {
    /// Records why one attribute of an admitted item is unknown. A later record of the
    /// same attribute replaces the earlier one in later publications. A reason that is
    /// not an [`attribute_reason`] constant is not kept, so the result reports the
    /// attribute as unknown without a registered reason.
    ///
    /// # Errors
    /// `Unavailable` after the build finished and `InvalidTree` when the root and path
    /// do not name an admitted item.
    pub fn set_attribute_unknown(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        attribute: ObservedAttribute,
        reason: &str,
    ) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        if !self.valid_root(root) {
            return Err(BuildError::InvalidTree);
        }
        let node = self
            .observations
            .locate(root, path)
            .ok_or(BuildError::InvalidTree)?;
        if let Some(code) = code(attribute, reason) {
            self.observations.record_attribute_reason(node, code);
        }
        Ok(())
    }
}

/// The unknown reasons of one publication, ascending by item and attribute.
#[derive(Debug, Default)]
pub(crate) struct AttributeReasons {
    items: Box<[u32]>,
    codes: Box<[u8]>,
}

impl AttributeReasons {
    /// Binds the rows recorded by store node to the item numbers of a publication; the
    /// last record of an attribute wins. Linear in the rows plus a sort.
    pub(crate) fn bind(item_of_node: &[u32], recorded: &[(u32, u8)]) -> Self {
        // Newest first, so a stable sort keeps the newest row of each attribute first.
        let mut rows: Vec<(u32, u8)> = recorded
            .iter()
            .rev()
            .filter_map(|(node, code)| Some((*item_of_node.get(*node as usize)?, *code)))
            .collect();
        rows.sort_by_key(|(item, code)| (*item, attribute_of(*code)));
        rows.dedup_by(|later, kept| {
            later.0 == kept.0 && attribute_of(later.1) == attribute_of(kept.1)
        });
        Self {
            items: rows.iter().map(|row| row.0).collect(),
            codes: rows.iter().map(|row| row.1).collect(),
        }
    }

    fn reason(&self, item: u32, attribute: ObservedAttribute) -> Option<&'static str> {
        let start = self.items.partition_point(|row| *row < item);
        let rows = self
            .items
            .get(start..)?
            .iter()
            .zip(self.codes.get(start..)?);
        rows.take_while(|(row, _)| **row == item)
            .find(|(_, code)| attribute_of(**code) == attribute as u8)
            .and_then(|(_, code)| reason_of(*code))
    }

    fn heap_bytes(&self) -> usize {
        self.items.len() * std::mem::size_of::<u32>() + self.codes.len()
    }
}

impl ResultVersion {
    /// Why an attribute of an item is unknown: one of the [`attribute_reason`]
    /// constants. `None` when the attribute was known, does not apply to the item's
    /// kind, or the observation gave no registered reason.
    pub fn attribute_unknown_reason(
        &self,
        item_no: u32,
        attribute: ObservedAttribute,
    ) -> Option<&'static str> {
        self.sparse.attribute_reasons.reason(item_no, attribute)
    }

    /// Heap bytes this version holds for unknown attribute reasons: five per row.
    pub fn attribute_reason_bytes(&self) -> usize {
        self.sparse.attribute_reasons.heap_bytes()
    }
}

#[cfg(test)]
mod tests;
