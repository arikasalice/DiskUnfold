//! The reason keys of a model's remainder, and the relation between its totals.

/// The remainder holds siblings that were left out because they are below the minimum
/// shape area or lack coordinate precision.
pub const REMAINDER_TOO_SMALL: &str = "viz.remainder.too_small";

/// The remainder holds space that no single item can carry, such as a file shared between
/// siblings that the scope counts once.
pub const REMAINDER_UNATTRIBUTED: &str = "viz.remainder.unattributed";
