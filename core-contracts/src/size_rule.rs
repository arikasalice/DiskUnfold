//! Published identifiers of the size aggregation rule family (F03).
//!
//! The registry that maps these IDs to implementations lives in `analysis-index`;
//! the identifiers sit here so that composition and settings code can name them
//! without depending on an implementation.

/// Algorithm kind under which size aggregation rules are registered.
pub const SIZE_RULE_KIND: &str = "size_rule";

/// ID of the rule a registry must provide and a build uses when none is bound.
pub const DEFAULT_SIZE_RULE_ID: &str = "size.agg.subtree-unique";

/// Reason key for a size aggregation rule ID that no registry entry carries.
///
/// It travels as `INVALID_ARGUMENT` when an analysis start names such an ID.
pub const SPACE_RULE_UNKNOWN: &str = "space.rule_unknown";
