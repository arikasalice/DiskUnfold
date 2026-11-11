//! Registered internal F03 aggregation implementation and its result identity.
//!
//! This is not the shared strategy contract or a settings-based selection API.
//! Only the implemented subtree rule is bound; first-path is not registered here.
use crate::{ResultVersion, physical, query::SpaceTotalError, space, tree::BuiltItems};
use du_core_contracts::ItemRef;
use du_core_contracts::result::{ResultCompleteness, SpaceAggregate};

/// Identity of the implementation that produced a result version's aggregates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeRuleIdentity {
    /// Published algorithm identifier.
    pub id: &'static str,
    /// Algorithm revision, independent of result and settings revisions.
    pub revision: u32,
}

/// A build-bound implementation, selected before observations are published.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RegisteredRule {
    SubtreeUnique,
}

// The single entry is the complete set of currently implemented bindings.
const BUILTIN_RULES: [RegisteredRule; 1] = [RegisteredRule::SubtreeUnique];

impl RegisteredRule {
    /// Returns the built-in implementation used by the current build path.
    pub(crate) const fn default_registration() -> Self {
        BUILTIN_RULES[0]
    }

    pub(crate) const fn identity(self) -> SizeRuleIdentity {
        match self {
            Self::SubtreeUnique => SizeRuleIdentity {
                id: "size.agg.subtree-unique",
                revision: 1,
            },
        }
    }

    pub(crate) fn aggregate(
        self,
        built: &BuiltItems,
        completeness: ResultCompleteness,
        limit: usize,
    ) -> Vec<SpaceAggregate> {
        match self {
            Self::SubtreeUnique => {
                let mut aggregates =
                    space::aggregate(&built.items, &built.structures, completeness);
                physical::apply(
                    &built.items,
                    &built.structures,
                    built.physical.as_deref(),
                    completeness,
                    limit,
                    &mut aggregates,
                );
                aggregates
            }
        }
    }

    pub(crate) fn scope_total(
        self,
        built: &BuiltItems,
        completeness: ResultCompleteness,
        limit: usize,
    ) -> SpaceAggregate {
        match self {
            Self::SubtreeUnique => {
                let mut total = space::scope_total(&built.items, &built.structures, completeness);
                physical::scope_total(
                    &built.items,
                    &built.structures,
                    built.physical.as_deref(),
                    completeness,
                    limit,
                    &mut total,
                );
                total
            }
        }
    }

    /// Applies the pinned rule to the observed members of a materialized search.
    pub(crate) fn selection_total(
        self,
        version: &ResultVersion,
        items: &[ItemRef],
        membership_complete: bool,
        cancelled: impl FnMut() -> bool,
    ) -> Result<SpaceAggregate, SpaceTotalError> {
        match self {
            Self::SubtreeUnique => {
                space::selection::sum(version, items, membership_complete, cancelled)
            }
        }
    }
}
