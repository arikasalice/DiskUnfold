//! The scan state of one folder, shared by folder rows and by item details.
//!
//! Kept below both `browse_ipc` and `browse_page_ipc` so that a detail can carry the
//! same value as its row without a module cycle (F02 #347).
use crate::{ItemRef, OsPath, browse_value::BrowseValue};
use serde::{Deserialize, Serialize};

/// Why a folder was not entered under the scope rules, an open set of snake_case
/// strings.
///
/// Known names are `other_volume` and `already_counted`. A later contract version may
/// add names. Deserialization accepts any string, and a receiver must never treat a
/// name it does not know as a folder that was read; it shows the folder as not
/// entered.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
#[serde(transparent)]
pub struct FolderBoundary(String);

impl FolderBoundary {
    /// The folder is on another volume than its parent and the scope does not cross
    /// that boundary.
    pub const OTHER_VOLUME: &'static str = "other_volume";
    /// The folder was already collected at another place of the same result.
    pub const ALREADY_COUNTED: &'static str = "already_counted";

    /// Wraps a boundary name without checking it against the known names.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the boundary name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Independent facts about a folder-query row, each with its own evidence.
///
/// The reason key of an unknown value says why the fact is not decided.
/// `scan.in_progress` means the scan that produced this result version has not ended
/// (also while it is paused), so the value is not decided yet; a screen leaves the
/// cell empty. `space.data_insufficient` means this result version does not hold the
/// information. These keys travel inside values only: `scan.in_progress` is not an
/// error-envelope reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FolderFlags {
    /// Present with `true` when the contents of this folder should have been read but
    /// could not be: its open or a read failed, its boundary could not be decided, or
    /// the scan was cancelled or its volume was disconnected first. A boundary that
    /// could not be decided marks the row of the folder that was not entered, not its
    /// parent. A table shows only this flag.
    /// A cancelled result alone does not establish why an individual folder was not read.
    ///
    /// A row never has `unenumerated` present with `true` together with a present
    /// `boundary`.
    pub unenumerated: BrowseValue<bool>,
    /// Whether an exclusion rule left this entry out. The collector does not keep
    /// excluded entries, so a row of a scan-lineage result is never an excluded entry
    /// and a producer may send present with `false`.
    pub excluded: BrowseValue<bool>,
    /// The scope rule that kept this folder from being entered.
    ///
    /// Present: the folder was not entered because of the scope rules. Absent: the
    /// folder is confirmed not to be a boundary. Unknown: it cannot be decided yet.
    /// Omitted: the source does not supply this fact, as for a source that is not
    /// scan lineage and for file rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub boundary: Option<BrowseValue<FolderBoundary>>,
    /// Where a folder that was `already_counted` through a confirmed firmlink was
    /// counted, such as `/Users` for `/System/Volumes/Data/Users` (#263).
    ///
    /// For navigation only: never add or subtract sizes with it. Present only with a
    /// present `already_counted` boundary whose cause is a firmlink of the confirmed
    /// group; omitted otherwise. `null` is rejected. GUI channel only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "non_null"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub counted_at: Option<OsPath>,
    /// The item that `counted_at` names in the same result version, for moving there
    /// (#263). Present only with `counted_at` and only when that folder is in the
    /// result; never used for sizes. `null` is rejected. GUI channel only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "non_null"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub counted_at_item: Option<ItemRef>,
    /// The folder's place in the result's confirmed root volume group (#263), an
    /// open set: [`FolderFlags::GROUP_ROLE_DATA`] marks the data mount itself.
    /// Omitted for every other folder and an unknown value means no mark; the
    /// screen never compares paths for it. `null` is rejected. GUI channel only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "non_null"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub group_volume_role: Option<String>,
    /// Unread ranges inside a folder that was itself read (#226).
    ///
    /// Present: some ranges below the folder were not read; see [`UnreadInside`]. Absent:
    /// nothing below was recorded as unread. Unknown: the scan has not ended. Omitted: the
    /// fact does not apply, as for a folder that was not read itself (its row shows
    /// `unenumerated`), a file row or a source that is not scan lineage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub unread_inside: Option<BrowseValue<UnreadInside>>,
}

/// Unread ranges recorded below a folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UnreadInside {
    /// Number of unread ranges (gaps, not folders) below the folder, as a decimal string.
    /// Omitted when a folder below was left unread by a cancellation or disconnection,
    /// which is not counted as a range, so the number is not known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub count: Option<String>,
}

impl FolderFlags {
    /// [`FolderFlags::group_volume_role`] of the data mount of a root volume group.
    pub const GROUP_ROLE_DATA: &'static str = "data";
    /// Reason key of an unknown fact while the scan of the result version has not
    /// ended. It travels inside values only.
    pub const REASON_SCAN_IN_PROGRESS: &'static str = "scan.in_progress";
    /// Reason key of an unknown fact the result version does not hold.
    pub const REASON_DATA_INSUFFICIENT: &'static str = "space.data_insufficient";
}

/// An optional field that, when present, must not be `null`.
pub(crate) fn non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
