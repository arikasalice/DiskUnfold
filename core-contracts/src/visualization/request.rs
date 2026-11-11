//! F06 view lifecycle and render admission wire values.
use super::{
    geometry::COORDINATE_MAX,
    optional_present,
    render::{VizColorMode, VizLabelMode},
    rows::VizDenominatorBasis,
};
use crate::{
    Id, ItemRef, SizeBasis, browse_open_ipc::QueryCondition, browse_page_ipc::QueryStrategy,
};
use serde::{Deserialize, Deserializer, Serialize, de::Error};
use std::{collections::HashSet, num::NonZeroU32};

/// Opens a view owned by the calling desktop session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "Record<string, never>"))]
#[serde(deny_unknown_fields)]
pub struct VizViewOpenRequest {}

/// The opaque channel allocated to the caller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VizViewChannel {
    pub view_channel: Id,
}

/// A source reference interpreted by the registered owner of its kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct VizSourceRef {
    pub kind: String,
    #[serde(rename = "ref")]
    pub reference: Id,
}

/// A normalized filter or a reference to an already evaluated query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum VizCondition {
    Normalized { condition: QueryCondition },
    QueryResult { query_ref: Id },
}

/// Logical viewport dimensions and the display scale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct VizViewport {
    pub width: NonZeroU32,
    pub height: NonZeroU32,
    #[serde(deserialize_with = "positive_scale")]
    pub scale: f64,
}

/// A positive rectangle in normalized model coordinates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct VizVisibleWindow {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

/// A request to reveal additional descendants of an item in the same result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VizExpansion {
    pub item_ref: ItemRef,
    pub extra_depth: u32,
}

/// Render inputs before resolving their source, strategy and view policy.
///
/// The engine must resolve every reference against the admitted result and
/// validate the injected depth and scale limits before replacing a subscription.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VizRenderRequest {
    pub view_channel: Id,
    pub request_seq: NonZeroU32,
    pub format: String,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub source_ref: Option<VizSourceRef>,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub result_revision: Option<Id>,
    pub aggregate_scope_ref: ItemRef,
    pub navigation_scope_ref: ItemRef,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub focus_item_ref: Option<ItemRef>,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub condition: Option<VizCondition>,
    pub measure: String,
    pub size_basis: SizeBasis,
    pub classification_criterion: String,
    pub color_criterion: String,
    pub denominator_basis: VizDenominatorBasis,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub strategy: Option<QueryStrategy>,
    pub viewport: VizViewport,
    #[serde(deserialize_with = "visible_window")]
    pub visible_window: VizVisibleWindow,
    #[serde(deserialize_with = "area")]
    pub min_area_px2: u32,
    pub depth_limit: u32,
    #[serde(
        default,
        deserialize_with = "expansions",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub expanded_depth_by_item: Option<Vec<VizExpansion>>,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub color_mode: Option<VizColorMode>,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub label_mode: Option<VizLabelMode>,
}

impl VizRenderRequest {
    /// Checks that explicit item references match the result selected by the engine.
    pub fn validate_revision(&self, revision: &Id) -> Result<(), &'static str> {
        let items = [&self.aggregate_scope_ref, &self.navigation_scope_ref]
            .into_iter()
            .chain(self.focus_item_ref.as_ref())
            .chain(
                self.expanded_depth_by_item
                    .iter()
                    .flatten()
                    .map(|entry| &entry.item_ref),
            );
        if self
            .result_revision
            .as_ref()
            .is_some_and(|value| value != revision)
            || items
                .into_iter()
                .any(|item| &item.result_version != revision)
        {
            return Err("render request refers to a different result revision");
        }
        Ok(())
    }
}

fn positive_scale<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let scale = f64::deserialize(deserializer)?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err(D::Error::custom("invalid visualization scale"));
    }
    Ok(scale)
}

fn visible_window<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<VizVisibleWindow, D::Error> {
    let window = VizVisibleWindow::deserialize(deserializer)?;
    if window.left >= window.right
        || window.top >= window.bottom
        || window.right > COORDINATE_MAX
        || window.bottom > COORDINATE_MAX
    {
        return Err(D::Error::custom("invalid visualization window"));
    }
    Ok(window)
}

fn area<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(deserializer)?;
    if !(4..=256).contains(&value) {
        return Err(D::Error::custom("invalid visualization area"));
    }
    Ok(value)
}

fn expansions<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<VizExpansion>>, D::Error> {
    let entries = Vec::<VizExpansion>::deserialize(deserializer)?;
    let mut seen = HashSet::new();
    if entries.len() > 64
        || entries.iter().any(|entry| {
            entry.extra_depth > 8
                || !seen.insert((&entry.item_ref.result_version, entry.item_ref.item_no))
        })
    {
        return Err(D::Error::custom("invalid visualization expansions"));
    }
    Ok(Some(entries))
}
