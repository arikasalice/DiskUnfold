//! Read-only, pinned result versions. Aggregation and query contracts follow separately.
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

/// An immutable snapshot held by the reader independently of later publications.
pub trait ResultVersionRead: Send + Sync {
    fn version_id(&self) -> &Id;
    fn scope_version_id(&self) -> &Id;
    fn completeness(&self) -> ResultCompleteness;
    fn items(&self) -> &[ResultItem];
}
