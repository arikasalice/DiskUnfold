//! Per-item presentation evidence proven by the pinned F03 result.
use du_core_contracts::{
    SizeAmount, SizeBasis, SizeEvidence, SizeValue, result::ResultItem,
    visualization::model::VizMatchKey,
};

use super::{facts::row_size, policy};
use crate::{
    current_analysis::CurrentAnalysis, viz_pages::session::ChildPolicyEvidence,
    viz_runtime::ItemEvidence,
};

/// Evidence for one actual child of the pinned folder.
///
/// The path comparison key is absent because the pinned result retains none, and the
/// match key is unknown for the same reason F05 browse rows report it: the result
/// holds no per-item volume and file identity. Neither is built from names or paths.
pub(super) fn child(
    source: &CurrentAnalysis,
    item: &ResultItem,
    basis: &SizeBasis,
) -> ItemEvidence {
    let size = row_size(source.version(), item.item_no, basis);
    ItemEvidence {
        presentation: ChildPolicyEvidence {
            path_compare_key: None,
            legend_class_key: policy::LEGEND_CLASS_NONE.to_owned(),
            display_flags: size_flags(size),
        },
        match_key: VizMatchKey::Unknown {
            reason_key: policy::MATCH_KEY_REASON.to_owned(),
        },
    }
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
