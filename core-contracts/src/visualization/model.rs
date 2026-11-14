//! Model-local references shared by visualization strategies and consumers.

use super::{
    geometry::Geometry,
    optional_present,
    quantity::{VizAmount, VizQuantity},
    render::{VizColorMode, VizLabelMode},
    required_nullable,
    rows::{VizDenominator, VizDenominatorBasis},
};
use crate::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeValue,
    browse_open_ipc::QueryCondition,
    browse_page_ipc::{CorrespondenceKey, QueryStrategy, ResultOrigin},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Hierarchy fields supplied together by new treemap models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ShapeMeta {
    /// Opaque, unique identifier scoped to this model.
    pub shape_ref: String,
    /// Closest returned ancestor, or null when none was returned.
    pub parent_shape_ref: Option<String>,
    /// Depth from the navigation root, including any omitted ancestors.
    pub visible_depth: u32,
    /// First folder in the branch, from the same result revision.
    pub root_branch_ref: Option<ItemRef>,
}

/// An open relationship kind between shapes within one model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizRelationship {
    /// Registered key preserved even when the consumer has no matching style.
    pub kind: String,
    pub from_shape_ref: String,
    pub to_shape_ref: String,
    /// Drawing information, when the strategy supplies geometry for the relationship.
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub geometry: Option<Geometry>,
}

/// A pinned visualization model and the evidence used to construct it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizModel {
    pub model_ref: String,
    pub condition_fingerprint: String,
    pub aggregate_scope_ref: ItemRef,
    pub navigation_scope_ref: ItemRef,
    #[serde(deserialize_with = "required_nullable")]
    pub focus_item_ref: Option<ItemRef>,
    pub origin: ResultOrigin,
    /// Data source of the pinned version; absent means a sample (F32 FR2.HD2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub data_source: Option<crate::data_source::DataSource>,
    pub staleness: VizStaleness,
    pub representation: VizRepresentation,
    pub denominator: VizDenominator,
    pub totals: VizTotals,
    pub shapes: Vec<VizShape>,
    pub relationships: Vec<VizRelationship>,
    pub legend: Vec<VizLegendEntry>,
    pub summary: VizSummary,
    pub strategy: QueryStrategy,
    pub table_revisions: VizTableRevisions,
}

/// Freshness evidence; unknown retains a reason supplied by the source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizStaleness {
    pub state: VizStalenessState,
    #[serde(deserialize_with = "required_nullable")]
    pub reason_key: Option<String>,
}

/// Freshness state of a result at the time the model was produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum VizStalenessState {
    Current,
    Stale,
    Unknown,
}

/// Actual registered representation selected for this model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizRepresentation {
    pub format: String,
    pub measure: String,
    pub size_basis: SizeBasis,
    pub classification_criterion: String,
    pub color_criterion: String,
    pub denominator_basis: VizDenominatorBasis,
    #[serde(deserialize_with = "required_nullable")]
    pub color_mode: Option<VizColorMode>,
    #[serde(deserialize_with = "required_nullable")]
    pub label_mode: Option<VizLabelMode>,
}

/// Quantity and optional original size evidence for one total.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizTotal {
    pub quantity: VizQuantity,
    #[serde(deserialize_with = "required_nullable")]
    pub source_size: Option<SizeValue>,
}

/// Separate aggregate, rendered, grouped, and filtered quantities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizTotals {
    pub scope: VizTotal,
    pub drawn: VizTotal,
    pub grouped: VizTotal,
    pub filtered_out: VizTotal,
}

/// Revisions of the tables and size rule fixed for the model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizTableRevisions {
    pub interpretation: u32,
    pub required_data: u32,
    pub legend: u32,
    pub view_policy: u32,
    pub size_rule: QueryStrategy,
}

/// Source-proven identity correspondence, including explicit unavailable states.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum VizMatchKey {
    Present { value: CorrespondenceKey },
    Unknown { reason_key: String },
    NotApplicable {},
}

/// One model shape referring to an item or a registered group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizShape {
    #[serde(deserialize_with = "required_nullable")]
    pub item_ref: Option<ItemRef>,
    #[serde(deserialize_with = "required_nullable")]
    pub group_ref: Option<String>,
    pub match_key: VizMatchKey,
    pub geometry: Geometry,
    pub size: SizeValue,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub quantity: Option<VizQuantity>,
    pub legend_class_key: String,
    pub display_flags: Vec<String>,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub shape_ref: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub parent_shape_ref: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub visible_depth: Option<u32>,
    #[serde(
        default,
        deserialize_with = "optional_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub root_branch_ref: Option<Option<ItemRef>>,
}

/// A legend class with its original item and quantity evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizLegendEntry {
    pub key: String,
    pub label_key: String,
    pub item_count: VizQuantity,
    pub total: VizTotal,
    #[serde(deserialize_with = "required_nullable")]
    pub condition: Option<QueryCondition>,
}

/// Explicit counts for grouped, omitted and unknown items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizSummary {
    pub grouped_items: VizQuantity,
    pub omitted_items: VizQuantity,
    pub unknown_items: VizQuantity,
    pub reason_keys: Vec<String>,
}

fn optional_nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

impl VizModel {
    /// Validates local structural and revision invariants. Group membership and source
    /// evidence require the pinned producer and cannot be established by this method.
    pub fn validate_revision(&self, revision: &Id) -> Result<(), &'static str> {
        if self.model_ref.is_empty()
            || self.condition_fingerprint.is_empty()
            || self.aggregate_scope_ref.result_version != *revision
            || self.navigation_scope_ref.result_version != *revision
            || self
                .focus_item_ref
                .as_ref()
                .is_some_and(|x| x.result_version != *revision)
        {
            return Err("invalid model identity or scope revision");
        }
        let rep = &self.representation;
        if [
            &rep.format,
            &rep.measure,
            &rep.classification_criterion,
            &rep.color_criterion,
        ]
        .iter()
        .any(|s| s.is_empty())
            || rep.denominator_basis != self.denominator.basis
            || (rep.format != "treemap" && (rep.color_mode.is_some() || rep.label_mode.is_some()))
        {
            return Err("invalid representation");
        }
        if match self.staleness.state {
            VizStalenessState::Unknown => self
                .staleness
                .reason_key
                .as_ref()
                .is_none_or(String::is_empty),
            _ => self.staleness.reason_key.is_some(),
        } {
            return Err("invalid staleness");
        }
        self.validate_rules()?;
        self.validate_totals()?;
        self.validate_shapes(revision)
    }

    fn validate_rules(&self) -> Result<(), &'static str> {
        let r = &self.table_revisions;
        if [
            r.interpretation,
            r.required_data,
            r.legend,
            r.view_policy,
            r.size_rule.revision,
            self.strategy.revision,
        ]
        .contains(&0)
            || r.size_rule.id.is_empty()
            || self.strategy.id.is_empty()
        {
            return Err("invalid table or strategy revision");
        }
        if [
            &self.summary.grouped_items,
            &self.summary.omitted_items,
            &self.summary.unknown_items,
        ]
        .iter()
        .any(|q| q.unit() != "items")
            || self.summary.reason_keys.iter().any(String::is_empty)
            || self.legend.iter().any(|l| {
                l.key.is_empty() || l.label_key.is_empty() || l.item_count.unit() != "items"
            })
        {
            return Err("invalid legend or summary");
        }
        Ok(())
    }

    fn validate_totals(&self) -> Result<(), &'static str> {
        let totals = [
            &self.totals.scope,
            &self.totals.drawn,
            &self.totals.grouped,
            &self.totals.filtered_out,
        ]
        .into_iter()
        .chain(self.legend.iter().map(|entry| &entry.total));
        let size_basis =
            (self.representation.measure == "size").then_some(&self.representation.size_basis);
        if totals.into_iter().any(|total| !total.matches(size_basis)) {
            return Err("model total does not match its source size");
        }
        Ok(())
    }

    fn validate_shapes(&self, revision: &Id) -> Result<(), &'static str> {
        let mut refs = HashMap::new();
        for shape in &self.shapes {
            shape.validate(revision, self.representation.format == "treemap")?;
            if let Some(key) = &shape.shape_ref
                && (key.is_empty() || refs.insert(key.as_str(), shape).is_some())
            {
                return Err("duplicate or empty shape reference");
            }
        }
        self.validate_parent_links(&refs)?;
        if self.relationships.iter().any(|r| {
            r.kind.is_empty()
                || !refs.contains_key(r.from_shape_ref.as_str())
                || !refs.contains_key(r.to_shape_ref.as_str())
        }) {
            return Err("invalid relationship");
        }
        Ok(())
    }

    fn validate_parent_links(&self, refs: &HashMap<&str, &VizShape>) -> Result<(), &'static str> {
        let mut checked = HashSet::new();
        let mut path = HashSet::new();
        for shape in &self.shapes {
            let mut child = shape;
            while let Some(key) = child.parent_shape_ref.as_ref().and_then(Option::as_deref) {
                let parent = *refs.get(key).ok_or("unknown parent shape")?;
                if matches!((child.visible_depth, parent.visible_depth), (Some(c), Some(p)) if p >= c)
                {
                    return Err("parent shape must be shallower than its child");
                }
                if checked.contains(key) {
                    break;
                }
                if !path.insert(key) {
                    return Err("cyclic shape hierarchy");
                }
                child = parent;
            }
            checked.extend(path.drain());
        }
        Ok(())
    }
}

impl VizShape {
    fn validate(&self, revision: &Id, treemap: bool) -> Result<(), &'static str> {
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
}

impl VizTotal {
    fn matches(&self, basis: Option<&SizeBasis>) -> bool {
        match (&self.source_size, basis) {
            (None, None) => true,
            (Some(size), Some(basis)) => {
                &size.basis == basis
                    && self.quantity.unit() == "bytes"
                    && match (&size.amount, self.quantity.amount()) {
                        (SizeAmount::Exact { bytes }, VizAmount::Exact { value })
                        | (SizeAmount::LowerBound { bytes }, VizAmount::LowerBound { value }) => {
                            bytes.as_str().parse::<i128>() == Ok(value.get())
                        }
                        (SizeAmount::Unknown, VizAmount::Unknown { .. }) => true,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
