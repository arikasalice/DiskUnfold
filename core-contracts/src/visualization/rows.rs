//! Shared F06 row and positional-page wire contracts.
use super::quantity::VizQuantity;
use crate::{
    Id, ItemRef, SizeAmount, SizeEvidence, SizeValue, Timestamp, result::ResultCompleteness,
};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::collections::HashSet;

/// Maximum rows in one positional visualization page.
pub const MAX_PAGE_COUNT: u32 = 1000;

/// Canonical signed decimal ratio, with at most 20 fractional digits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS), ts(type = "string"))]
#[serde(transparent)]
pub struct VizRatio(String);
impl VizRatio {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        let unsigned = value.strip_prefix('-').unwrap_or(value);
        let mut parts = unsigned.split('.');
        let integer = parts.next().unwrap_or("");
        let fractional = parts.next();
        let valid_integer = !integer.is_empty()
            && integer.bytes().all(|b| b.is_ascii_digit())
            && (integer == "0" || !integer.starts_with('0'));
        let valid_fraction = fractional.is_none_or(|f| {
            !f.is_empty()
                && f.len() <= 20
                && f.bytes().all(|b| b.is_ascii_digit())
                && !f.ends_with('0')
        });
        if !valid_integer || !valid_fraction || parts.next().is_some() || value == "-0" {
            return Err("invalid canonical ratio");
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for VizRatio {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(D::Error::custom)
    }
}
/// Published denominator choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum VizDenominatorBasis {
    ParentFolder,
    Scope,
    AnalysisTotal,
}

/// Denominator quantity and optional original F03 size evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizDenominator {
    pub basis: VizDenominatorBasis,
    pub value: VizQuantity,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub size: Option<SizeValue>,
}
fn optional_present<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d).map(Some)
}
fn required_nullable<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d)
}

/// A display row shared by viz.rows and viz.group_rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizRow {
    pub item_ref: Option<ItemRef>,
    pub group_ref: Option<String>,
    pub display_name: String,
    pub size: SizeValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub quantity: Option<VizQuantity>,
    pub ratio: Option<VizRatio>,
    pub denominator: VizDenominator,
    pub legend_class_key: String,
    pub display_flags: Vec<String>,
}
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
}
impl VizRow {
    /// Checks local invariants. The caller must resolve group references in the pinned model.
    pub fn validate(&self) -> Result<(), &'static str> {
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
        Ok(())
    }
    /// Checks the item revision only; group membership requires the pinned model.
    pub fn validate_revision(&self, revision: &Id) -> Result<(), &'static str> {
        self.validate()?;
        if self
            .item_ref
            .as_ref()
            .is_some_and(|item| &item.result_version != revision)
        {
            return Err("row revision mismatch");
        }
        Ok(())
    }
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
        };
        row.validate().map_err(D::Error::custom)?;
        Ok(row)
    }
}
/// Position-based request; the engine resolves and checks the model and result revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VizRowsRequest {
    #[serde(deserialize_with = "nonempty")]
    pub model_ref: String,
    pub result_revision: Id,
    pub start: u32,
    #[serde(deserialize_with = "page_count")]
    pub count: u32,
}
impl VizRowsRequest {
    /// Checks only local request constraints; model lifetime and membership belong to the engine.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.model_ref.is_empty() || self.count == 0 || self.count > MAX_PAGE_COUNT {
            return Err("invalid rows request");
        }
        Ok(())
    }
}
/// Position-based request for members of one model-local group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VizGroupRowsRequest {
    #[serde(deserialize_with = "nonempty")]
    pub model_ref: String,
    #[serde(deserialize_with = "nonempty")]
    pub group_ref: String,
    pub start: u32,
    #[serde(deserialize_with = "positive_count")]
    pub count: u32,
}
impl VizGroupRowsRequest {
    /// Checks the published request fields; group membership is checked by the engine.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.model_ref.is_empty() || self.group_ref.is_empty() || self.count == 0 {
            return Err("invalid group rows request");
        }
        Ok(())
    }
}
fn positive_count<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(d)?;
    if value == 0 {
        return Err(D::Error::custom("count must be positive"));
    }
    Ok(value)
}
/// Page from one pinned model. totalCount is a JSON-safe integer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizRows {
    pub rows: Vec<VizRow>,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub total_count: u64,
    pub result_revision: Id,
    pub observed_at: Option<Timestamp>,
    pub completeness: ResultCompleteness,
}
impl VizRows {
    /// Checks the page's local row and count invariants, not model/group ownership.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.total_count >= (1_u64 << 53) || self.rows.len() as u64 > self.total_count {
            return Err("invalid row count");
        }
        for row in &self.rows {
            row.validate_revision(&self.result_revision)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RowsWire {
    rows: Vec<VizRow>,
    #[serde(deserialize_with = "safe_count")]
    total_count: u64,
    result_revision: Id,
    #[serde(deserialize_with = "required_nullable")]
    observed_at: Option<Timestamp>,
    completeness: ResultCompleteness,
}
impl<'de> Deserialize<'de> for VizRows {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = RowsWire::deserialize(d)?;
        let page = Self {
            rows: w.rows,
            total_count: w.total_count,
            result_revision: w.result_revision,
            observed_at: w.observed_at,
            completeness: w.completeness,
        };
        page.validate().map_err(D::Error::custom)?;
        Ok(page)
    }
}

/// Proven minimum and maximum size for every member of a group.
///
/// Unknown is used when any member is unobserved or uses a different size basis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum VizSizeRange {
    Known { min: SizeValue, max: SizeValue },
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SizeRangeWire {
    kind: String,
    #[serde(default, deserialize_with = "deserialize_range_bound")]
    min: RangeBound,
    #[serde(default, deserialize_with = "deserialize_range_bound")]
    max: RangeBound,
}

#[derive(Default)]
enum RangeBound {
    #[default]
    Missing,
    Present(SizeValue),
}

fn deserialize_range_bound<'de, D: Deserializer<'de>>(d: D) -> Result<RangeBound, D::Error> {
    SizeValue::deserialize(d).map(RangeBound::Present)
}

impl<'de> Deserialize<'de> for VizSizeRange {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = SizeRangeWire::deserialize(d)?;
        match (wire.kind.as_str(), wire.min, wire.max) {
            ("known", RangeBound::Present(min), RangeBound::Present(max)) => {
                Ok(Self::Known { min, max })
            }
            ("unknown", RangeBound::Missing, RangeBound::Missing) => Ok(Self::Unknown),
            ("known", _, _) => Err(D::Error::custom("known size range requires min and max")),
            ("unknown", _, _) => Err(D::Error::custom("unknown size range cannot contain bounds")),
            _ => Err(D::Error::custom("invalid size range kind")),
        }
    }
}

/// One page of members and aggregate evidence from a fixed visualization result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizGroupRows {
    pub rows: Vec<VizRow>,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub total_count: u64,
    pub group_total: SizeValue,
    pub size_range: VizSizeRange,
    pub result_revision: Id,
    pub observed_at: Option<Timestamp>,
    pub completeness: ResultCompleteness,
}

impl VizGroupRows {
    /// Checks page size, JSON-safe count, and item revisions; group ownership is model-bound.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.total_count >= (1_u64 << 53) || self.rows.len() as u64 > self.total_count {
            return Err("invalid group row count");
        }
        for row in &self.rows {
            row.validate_revision(&self.result_revision)?;
        }
        if let VizSizeRange::Known { min, max } = &self.size_range {
            if min.basis != max.basis || min.basis != self.group_total.basis {
                return Err("mixed size range basis");
            }
            let (SizeAmount::Exact { bytes: low }, SizeAmount::Exact { bytes: high }) =
                (&min.amount, &max.amount)
            else {
                return Err("inexact size range");
            };
            if min.evidence == SizeEvidence::Unknown || max.evidence == SizeEvidence::Unknown {
                return Err("unknown size range evidence");
            }
            let low = low.as_str().parse::<u64>().map_err(|_| "invalid minimum")?;
            let high = high
                .as_str()
                .parse::<u64>()
                .map_err(|_| "invalid maximum")?;
            if low > high {
                return Err("reversed size range");
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupRowsWire {
    rows: Vec<VizRow>,
    #[serde(deserialize_with = "safe_count")]
    total_count: u64,
    group_total: SizeValue,
    size_range: VizSizeRange,
    result_revision: Id,
    #[serde(deserialize_with = "required_nullable")]
    observed_at: Option<Timestamp>,
    completeness: ResultCompleteness,
}

impl<'de> Deserialize<'de> for VizGroupRows {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = GroupRowsWire::deserialize(d)?;
        let page = Self {
            rows: w.rows,
            total_count: w.total_count,
            group_total: w.group_total,
            size_range: w.size_range,
            result_revision: w.result_revision,
            observed_at: w.observed_at,
            completeness: w.completeness,
        };
        page.validate().map_err(D::Error::custom)?;
        Ok(page)
    }
}

fn nonempty<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = String::deserialize(d)?;
    if value.is_empty() {
        return Err(D::Error::custom("empty model reference"));
    }
    Ok(value)
}
fn page_count<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let count = u32::deserialize(d)?;
    if count == 0 || count > MAX_PAGE_COUNT {
        return Err(D::Error::custom("invalid page count"));
    }
    Ok(count)
}
fn safe_count<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let count = u64::deserialize(d)?;
    if count >= (1_u64 << 53) {
        return Err(D::Error::custom("invalid total count"));
    }
    Ok(count)
}
