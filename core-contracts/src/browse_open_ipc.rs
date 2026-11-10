//! Published GUI browse-open admission and normalized condition shapes.
use crate::{ByteSize, Id, ItemRef, PathBytes, SizeBasis};
use serde::{Deserialize, Serialize};

/// A result source selected by kind and opaque reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowseSourceRef {
    pub kind: BrowseSourceKind,
    #[serde(rename = "ref")]
    pub reference: Id,
}

/// Published result source kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum BrowseSourceKind {
    Scan,
    Snapshot,
    Catalog,
    Query,
}

/// Folder list or one expanded tree branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum BrowseViewKind {
    List,
    TreeBranch,
}

/// One registered sort key, in request priority order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowseSortKey {
    pub id: String,
    pub revision: u32,
    pub descending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_basis: Option<SizeBasis>,
}

/// A condition tree shared by search and folder browsing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryCondition {
    All { children: Vec<QueryCondition> },
    Any { children: Vec<QueryCondition> },
    Not { child: Box<QueryCondition> },
    Predicate { predicate: QueryPredicate },
}

/// Supported baseline condition operands; registered operands remain opaque.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryPredicate {
    Name {
        op: QueryNameOp,
        encoding: QueryNameEncoding,
        value: PathBytes,
    },
    Type {
        value: QueryItemType,
    },
    Size {
        basis: QuerySizeBasis,
        op: QuerySizeOp,
        bytes: ByteSize,
    },
    Registered {
        id: String,
        revision: u32,
        operand: PathBytes,
    },
}

/// Name matching operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QueryNameOp {
    Equal,
    Contains,
    StartsWith,
    EndsWith,
}
/// Encoding of the name operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QueryNameEncoding {
    Bytes,
    WindowsUtf16le,
}
/// Item kinds supported by the baseline type predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QueryItemType {
    File,
    Directory,
    Link,
}
/// Size bases accepted by the baseline size predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QuerySizeBasis {
    Logical,
    Allocated,
}
/// Inclusive and exclusive size comparisons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QuerySizeOp {
    AtLeast,
    GreaterThan,
    AtMost,
    LessThan,
}

/// Opens one folder; correlation ID remains in the outer call envelope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowseOpenRequest {
    pub view_kind: BrowseViewKind,
    pub request_seq: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<BrowseSourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_version: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<ItemRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_keys: Option<Vec<BrowseSortKey>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<QueryCondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
}

#[cfg(test)]
mod tests;
