//! The three states of an observed value, shared by every browse-style contract.
use serde::{Deserialize, Serialize};

/// Distinguishes an absent value from a value the source could not establish.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum BrowseValue<T> {
    Present { value: T },
    Absent,
    Unknown { reason_key: String },
}
