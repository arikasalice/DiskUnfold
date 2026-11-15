//! Internal F03 aggregation implementations and the bound rule a build carries.
//!
//! Registration by ID and revision, and selection of a rule for a build, live in
//! the `size_rule_registry` module. This is not the shared strategy contract or a
//! settings-based selection API. Only the subtree rule is implemented so far.
//! A further rule is one new `RegisteredRule` variant plus one entry in
//! `BUILTIN_RULES`; callers keep selecting rules by ID through the registry.
use crate::{ResultVersion, physical, query::SpaceTotalError, space, tree::BuiltItems};
use du_core_contracts::ItemRef;
use du_core_contracts::result::{ResultCompleteness, SpaceAggregate};

pub use du_core_contracts::size_rule::DEFAULT_SIZE_RULE_ID;

/// Identity of the implementation that produced a result version's aggregates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SizeRuleIdentity {
    /// Published algorithm identifier.
    pub id: &'static str,
    /// Algorithm revision, independent of result and settings revisions.
    pub revision: u32,
}

/// A size aggregation rule obtained from a size rule registry.
///
/// The value is opaque and cheap to copy. It reports the identity it was
/// registered under, and a result version built with it reports that identity.
/// Callers cannot construct one; they bind it with `BuildHandle::set_size_rule`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundSizeRule {
    identity: SizeRuleIdentity,
    rule: RegisteredRule,
}

impl BoundSizeRule {
    /// Binds an implementation under the identity it declares.
    pub(crate) const fn bind(rule: RegisteredRule) -> Self {
        Self {
            identity: rule.identity(),
            rule,
        }
    }

    /// Binds an implementation under a different registered identity, for tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn bind_as(identity: SizeRuleIdentity, rule: RegisteredRule) -> Self {
        Self { identity, rule }
    }

    /// The rule a build uses when nothing was bound to it.
    pub(crate) const fn builtin_default() -> Self {
        Self::bind(RegisteredRule::SubtreeUnique)
    }

    /// Returns the identity this rule was registered under.
    pub const fn identity(self) -> SizeRuleIdentity {
        self.identity
    }

    pub(crate) fn aggregate(
        self,
        built: &BuiltItems,
        completeness: ResultCompleteness,
        limit: usize,
    ) -> Vec<SpaceAggregate> {
        self.rule.aggregate(built, completeness, limit)
    }

    pub(crate) fn scope_total(
        self,
        built: &BuiltItems,
        completeness: ResultCompleteness,
        limit: usize,
    ) -> SpaceAggregate {
        self.rule.scope_total(built, completeness, limit)
    }

    pub(crate) fn selection_total(
        self,
        version: &ResultVersion,
        items: &[ItemRef],
        membership_complete: bool,
        cancelled: impl FnMut() -> bool,
    ) -> Result<SpaceAggregate, SpaceTotalError> {
        self.rule
            .selection_total(version, items, membership_complete, cancelled)
    }

    pub(crate) fn selection_total_with_range(
        self,
        version: &ResultVersion,
        items: &[ItemRef],
        membership_complete: bool,
        cancelled: impl FnMut() -> bool,
    ) -> Result<(SpaceAggregate, crate::observation_range::RangeIndices), SpaceTotalError> {
        self.rule
            .selection_total_with_range(version, items, membership_complete, cancelled)
    }
}

/// An implemented aggregation algorithm, dispatched statically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegisteredRule {
    SubtreeUnique,
}

/// The default registration bundle: every implemented rule, each under its own identity.
pub(crate) const BUILTIN_RULES: &[RegisteredRule] = &[RegisteredRule::SubtreeUnique];

impl RegisteredRule {
    pub(crate) const fn identity(self) -> SizeRuleIdentity {
        match self {
            Self::SubtreeUnique => SizeRuleIdentity {
                id: DEFAULT_SIZE_RULE_ID,
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

    pub(crate) fn selection_total_with_range(
        self,
        version: &ResultVersion,
        items: &[ItemRef],
        membership_complete: bool,
        cancelled: impl FnMut() -> bool,
    ) -> Result<(SpaceAggregate, crate::observation_range::RangeIndices), SpaceTotalError> {
        match self {
            Self::SubtreeUnique => {
                space::selection::sum_with_range(version, items, membership_complete, cancelled)
            }
        }
    }
}
