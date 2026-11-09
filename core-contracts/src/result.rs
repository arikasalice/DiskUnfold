//! Read-only, pinned result versions and their conservative space aggregates.
use crate::{Id, SizeValue};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultItem {
    pub item_no: u32,
    pub parent: Option<u32>,
    pub name: Vec<u8>,
    pub size: Option<SizeValue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ResultCompleteness {
    Partial,
    Complete,
    Missing,
    Cancelled,
}

/// Space information derived solely from observations in one immutable result version.
///
/// `missing_items` counts observations lacking a usable logical value; it does not
/// include unobserved entries. The latter are represented by `completeness`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceAggregate {
    pub logical: SizeValue,
    pub allocated: SizeValue,
    pub unique_physical: SizeValue,
    pub completeness: ResultCompleteness,
    pub missing_items: u64,
    /// Machine-readable limits on what the observation input can establish.
    pub limitations: &'static [&'static str],
}

/// An immutable snapshot held by the reader independently of later publications.
pub trait ResultVersionRead: Send + Sync {
    fn version_id(&self) -> &Id;
    fn scope_version_id(&self) -> &Id;
    fn completeness(&self) -> ResultCompleteness;
    fn items(&self) -> &[ResultItem];
}
