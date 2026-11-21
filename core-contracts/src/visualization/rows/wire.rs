//! The wire form of a row: read as sent, then checked as a row.

use super::{
    VizDenominator, VizItemKind, VizLink, VizQuantity, VizRatio, VizRow, VizTypeSlice, VizUnread,
    optional_known, optional_present, required_nullable,
};
use crate::{ItemRef, SizeValue, browse_open_ipc::QueryCondition};
use serde::{Deserialize, Deserializer, de::Error as _};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RowWire {
    #[serde(deserialize_with = "required_nullable")]
    item_ref: Option<ItemRef>,
    #[serde(deserialize_with = "required_nullable")]
    group_ref: Option<String>,
    display_name: String,
    size: SizeValue,
    #[serde(default, deserialize_with = "optional_present")]
    quantity: Option<VizQuantity>,
    #[serde(deserialize_with = "required_nullable")]
    ratio: Option<VizRatio>,
    denominator: VizDenominator,
    legend_class_key: String,
    display_flags: Vec<String>,
    #[serde(default, deserialize_with = "optional_present")]
    label_key: Option<String>,
    #[serde(default, deserialize_with = "optional_present")]
    member_count: Option<VizQuantity>,
    #[serde(default, deserialize_with = "optional_known")]
    item_kind: Option<VizItemKind>,
    #[serde(default, deserialize_with = "optional_present")]
    condition: Option<QueryCondition>,
    #[serde(default, deserialize_with = "optional_present")]
    unread: Option<VizUnread>,
    #[serde(default, deserialize_with = "optional_present")]
    type_slices: Option<Vec<VizTypeSlice>>,
    #[serde(default, deserialize_with = "optional_present")]
    links: Option<Vec<VizLink>>,
}
impl<'de> Deserialize<'de> for VizRow {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = RowWire::deserialize(d)?;
        let row = Self {
            item_ref: w.item_ref,
            group_ref: w.group_ref,
            display_name: w.display_name,
            size: w.size,
            quantity: w.quantity,
            ratio: w.ratio,
            denominator: w.denominator,
            legend_class_key: w.legend_class_key,
            display_flags: w.display_flags,
            label_key: w.label_key,
            member_count: w.member_count,
            item_kind: w.item_kind,
            condition: w.condition,
            unread: w.unread,
            type_slices: w.type_slices,
            links: w.links,
        };
        row.validate().map_err(D::Error::custom)?;
        Ok(row)
    }
}
