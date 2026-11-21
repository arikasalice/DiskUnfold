use crate::item_kind::is_structure;
use du_core_contracts::result::{ResultCompleteness, ResultItem, ResultItemKind, SpaceAggregate};
use du_core_contracts::{ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue};

pub(crate) mod selection;

#[derive(Clone, Default)]
struct Accumulator {
    bytes: Option<u64>,
    missing: u64,
    untraversed_links: u64,
    /// A link whose own size is unknown; its value is never filled with zero.
    unsized_link: bool,
    lower_bound: bool,
    overflow: bool,
    evidence: Option<SizeEvidence>,
}

impl Accumulator {
    fn observe(&mut self, value: Option<&SizeValue>) {
        let Some(value) =
            value.filter(|v| v.basis == SizeBasis::Logical && v.evidence != SizeEvidence::Unknown)
        else {
            self.missing += 1;
            return;
        };
        let (bytes, lower) = match &value.amount {
            SizeAmount::Exact { bytes } => (bytes, false),
            SizeAmount::LowerBound { bytes } => (bytes, true),
            SizeAmount::Unknown => {
                self.missing += 1;
                return;
            }
        };
        self.merge_bytes(Some(bytes.get()));
        self.lower_bound |= lower;
        self.evidence = Some(weaker(self.evidence.take(), value.evidence.clone()));
    }

    /// Counts an untraversed link and adds the link object's own size when it is known.
    ///
    /// The target is outside the selected range and is only counted.
    fn observe_link(&mut self, value: Option<&SizeValue>) {
        self.untraversed_links += 1;
        if value.is_some_and(known_logical) {
            self.observe(value);
        } else {
            self.unsized_link = true;
        }
    }

    fn merge_bytes(&mut self, other: Option<u64>) {
        if let Some(other) = other {
            match self.bytes.unwrap_or(0).checked_add(other) {
                Some(sum) => self.bytes = Some(sum),
                None => self.overflow = true,
            }
        }
    }

    fn merge(&mut self, other: &Self) {
        self.merge_bytes(other.bytes);
        self.missing += other.missing;
        self.untraversed_links += other.untraversed_links;
        self.unsized_link |= other.unsized_link;
        self.lower_bound |= other.lower_bound;
        self.overflow |= other.overflow;
        if let Some(evidence) = &other.evidence {
            self.evidence = Some(weaker(self.evidence.take(), evidence.clone()));
        }
    }

    fn finish(&self, source: ResultCompleteness) -> SpaceAggregate {
        let completeness = match source {
            ResultCompleteness::Cancelled => ResultCompleteness::Cancelled,
            ResultCompleteness::Partial => ResultCompleteness::Partial,
            ResultCompleteness::Missing => ResultCompleteness::Missing,
            ResultCompleteness::Complete
                if self.missing > 0 || self.unsized_link || self.overflow || self.lower_bound =>
            {
                ResultCompleteness::Missing
            }
            ResultCompleteness::Complete => ResultCompleteness::Complete,
        };
        let amount = match (self.bytes, self.overflow, completeness) {
            (None, false, ResultCompleteness::Complete) => SizeAmount::Exact {
                bytes: ByteSize::from(0),
            },
            (_, true, _) | (None, _, _) => SizeAmount::Unknown,
            (Some(bytes), false, ResultCompleteness::Complete) => SizeAmount::Exact {
                bytes: ByteSize::from(bytes),
            },
            (Some(bytes), false, _) => SizeAmount::LowerBound {
                bytes: ByteSize::from(bytes),
            },
        };
        let evidence = if matches!(amount, SizeAmount::Unknown) {
            SizeEvidence::Unknown
        } else {
            self.evidence.clone().unwrap_or(SizeEvidence::Measured)
        };
        const LIMITATIONS: &[&str] = &[
            "space.allocated_not_collected",
            "space.sharing_not_collected",
            "space.aux_not_included",
            "space.observation_detail_insufficient",
        ];
        const OVERFLOW_LIMITATIONS: &[&str] = &[
            "space.allocated_not_collected",
            "space.sharing_not_collected",
            "space.aux_not_included",
            "space.observation_detail_insufficient",
            "space.aggregation_overflow",
        ];
        SpaceAggregate {
            logical: SizeValue {
                basis: SizeBasis::Logical,
                evidence,
                amount,
            },
            allocated: unknown(SizeBasis::Allocated),
            unique_physical: unknown(SizeBasis::UniquePhysical),
            completeness,
            allocated_completeness: ResultCompleteness::Missing,
            unique_physical_completeness: ResultCompleteness::Missing,
            missing_items: self.missing,
            untraversed_links: self.untraversed_links,
            inaccessible_ranges: None,
            limitations: if self.overflow {
                std::borrow::Cow::Borrowed(OVERFLOW_LIMITATIONS)
            } else {
                std::borrow::Cow::Borrowed(LIMITATIONS)
            },
        }
    }
}

fn unknown(basis: SizeBasis) -> SizeValue {
    SizeValue {
        basis,
        evidence: SizeEvidence::Unknown,
        amount: SizeAmount::Unknown,
    }
}

pub(crate) fn weaker(left: Option<SizeEvidence>, right: SizeEvidence) -> SizeEvidence {
    fn rank(value: &SizeEvidence) -> u8 {
        match value {
            SizeEvidence::Measured => 0,
            SizeEvidence::OsReported => 1,
            SizeEvidence::Estimated => 2,
            SizeEvidence::Unknown => 3,
        }
    }
    match left {
        Some(left) if rank(&left) >= rank(&right) => left,
        _ => right,
    }
}

pub(crate) fn aggregate(
    items: &[ResultItem],
    boundary: &[u32],
    completeness: ResultCompleteness,
    proven: Option<&crate::folder_completion::ProvenFolders>,
) -> Vec<SpaceAggregate> {
    let mut sums = vec![Accumulator::default(); items.len()];
    for item in items.iter().rev() {
        let index = item.item_no as usize;
        if item.kind == ResultItemKind::UntraversedLink {
            sums[index].observe_link(item.size.as_ref());
        } else if !is_structure(item.kind) {
            sums[index].observe(item.size.as_ref());
        }
        // A file outside the boundary keeps its own unknown value but is an unread range
        // for its folder, so it adds neither bytes nor a missing item above it.
        if let Some(parent) = item.parent
            && boundary.binary_search(&item.item_no).is_err()
        {
            let child = sums[index].clone();
            sums[parent as usize].merge(&child);
        }
    }
    let source =
        |index| crate::folder_completion::ProvenFolders::source(proven, index, completeness);
    sums.iter()
        .enumerate()
        .map(|(index, sum)| sum.finish(source(index)))
        .collect()
}

/// Computes a forest-wide total directly from observations, not subtree totals.
pub(crate) fn scope_total(
    items: &[ResultItem],
    boundary: &[u32],
    completeness: ResultCompleteness,
) -> SpaceAggregate {
    let mut sum = Accumulator::default();
    for item in items {
        if boundary.binary_search(&item.item_no).is_ok() {
            continue;
        }
        if item.kind == ResultItemKind::UntraversedLink {
            sum.observe_link(item.size.as_ref());
        } else if !is_structure(item.kind) {
            sum.observe(item.size.as_ref());
        }
    }
    sum.finish(completeness)
}

/// Whether a value is a usable logical size, the same test [`Accumulator::observe`] applies.
pub(crate) fn known_logical(value: &SizeValue) -> bool {
    value.basis == SizeBasis::Logical
        && value.evidence != SizeEvidence::Unknown
        && value.amount != SizeAmount::Unknown
}

/// Marks known unread ranges (#341).
///
/// What was read is a lower bound: an exact size becomes a lower bound, and a folder
/// read to no bytes at all is at least zero logical bytes. The unique physical size
/// stays unknown without bytes, as does any value of an overflowed aggregate.
pub(crate) fn mark_inaccessible(aggregate: &mut SpaceAggregate, count: u64, allocation: bool) {
    fn mark(value: &mut SizeValue, zero_is_bound: bool) {
        if let SizeAmount::Exact { bytes } = &value.amount {
            value.amount = if bytes.get() == 0 && !zero_is_bound {
                SizeAmount::Unknown
            } else {
                SizeAmount::LowerBound {
                    bytes: bytes.clone(),
                }
            };
            if value.amount == SizeAmount::Unknown {
                value.evidence = SizeEvidence::Unknown;
            }
        }
    }
    fn state(value: &mut ResultCompleteness) {
        if *value == ResultCompleteness::Complete {
            *value = ResultCompleteness::Missing;
        }
    }
    mark(&mut aggregate.logical, true);
    mark(&mut aggregate.allocated, true);
    mark(&mut aggregate.unique_physical, false);
    mark_unread(aggregate, allocation);
    state(&mut aggregate.completeness);
    state(&mut aggregate.allocated_completeness);
    state(&mut aggregate.unique_physical_completeness);
    aggregate.inaccessible_ranges = Some(count);
}

/// A folder whose contents were left unread, or one above it, is at least the
/// bytes read below it: with none read, at least zero (#341). The allocated size
/// follows only when this scan collected allocation (`allocation`); otherwise its
/// unknown means "not collected" and stays. An overflowed aggregate stays unknown,
/// and so does the unique physical size.
pub(crate) fn mark_unread(aggregate: &mut SpaceAggregate, allocation: bool) {
    if aggregate
        .limitations
        .contains(&"space.aggregation_overflow")
    {
        return;
    }
    let zero = |value: &mut SizeValue| {
        if value.amount == SizeAmount::Unknown {
            value.amount = SizeAmount::LowerBound {
                bytes: ByteSize::from(0),
            };
            value.evidence = SizeEvidence::Measured;
        }
    };
    zero(&mut aggregate.logical);
    if allocation {
        zero(&mut aggregate.allocated);
    }
}
