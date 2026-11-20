//! Per-item presentation evidence proven by the pinned F03 result.
use du_analysis_index::{query::NameEncoding, visualization::type_legend};
use du_core_contracts::{
    ItemRef, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue, result::ResultItem,
    visualization::model::VizMatchKey,
};

use super::{facts::row_size, policy};
use crate::{
    current_analysis::CurrentAnalysis, viz_pages::session::ChildPolicyEvidence,
    viz_runtime::ItemEvidence,
};

/// Evidence for one actual child of the pinned folder.
///
/// The path comparison key is not supplied; ordering does not use it. The match key is
/// unknown for the same reason F05 browse rows report it: the result holds no per-item
/// volume and file identity. Neither is built from names or paths.
///
/// With the type criterion the item's legend class is its type, decided by the type
/// classification table from its native name; otherwise every item shares one class.
pub(super) fn child(
    source: &CurrentAnalysis,
    item: &ResultItem,
    basis: &SizeBasis,
    by_type: bool,
) -> ItemEvidence {
    let size = row_size(source.version(), item.item_no, basis);
    let legend_class_key = if by_type {
        type_class(source, item)
    } else {
        policy::LEGEND_CLASS_UNCLASSIFIED
    };
    ItemEvidence {
        presentation: ChildPolicyEvidence {
            path_compare_key: None,
            legend_class_key: legend_class_key.to_owned(),
            display_flags: size_flags(size),
        },
        match_key: VizMatchKey::Unknown {
            reason_key: policy::MATCH_KEY_REASON.to_owned(),
        },
    }
}

/// The type class of an item, read in the encoding of its observed root. A name
/// whose root cannot be found has no type.
fn type_class(source: &CurrentAnalysis, item: &ResultItem) -> &'static str {
    let reference = ItemRef {
        result_version: source.result_version().clone(),
        item_no: item.item_no,
    };
    let Ok((_, root)) = source.version().observed_root_for(&reference) else {
        return type_legend::NO_CLASS;
    };
    let encoding = match root.path.encoding {
        PathEncoding::Unix => NameEncoding::Bytes,
        PathEncoding::WindowsUtf16le => NameEncoding::WindowsUtf16Le,
    };
    type_legend::class_of(item.kind, &item.name, encoding)
}

/// Flags only what the size value itself proves (HLD table 22: unknown, lower bound).
pub(super) fn size_flags(size: Option<&SizeValue>) -> Vec<String> {
    match size {
        Some(size) if size.evidence != SizeEvidence::Unknown => match size.amount {
            SizeAmount::Exact { .. } => Vec::new(),
            SizeAmount::LowerBound { .. } => vec![policy::FLAG_LOWER_BOUND.to_owned()],
            SizeAmount::Unknown => vec![policy::FLAG_UNKNOWN.to_owned()],
        },
        _ => vec![policy::FLAG_UNKNOWN.to_owned()],
    }
}
