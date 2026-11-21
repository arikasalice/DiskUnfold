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

#[cfg(test)]
pub(crate) fn accumulator_size_bytes() -> usize {
    std::mem::size_of::<Accumulator>()
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

/// Retains subtree aggregates only for structural items; leaf values are derived on read.
#[derive(Debug)]
pub(crate) struct SpaceAggregates {
    item_numbers: Vec<u32>,
    values: Vec<SpaceAggregate>,
    source_completeness: ResultCompleteness,
    promoted: Option<Vec<u64>>,
    file_gaps: Vec<(u32, u64)>,
}

impl SpaceAggregates {
    fn new(
        items: &[ResultItem],
        completeness: ResultCompleteness,
        proven: Option<&crate::folder_completion::ProvenFolders>,
    ) -> Self {
        let mut has_children = vec![false; items.len()];
        for item in items {
            if let Some(parent) = item.parent {
                has_children[parent as usize] = true;
            }
        }
        let item_numbers: Vec<_> = items
            .iter()
            .filter(|item| {
                is_structure(item.kind)
                    || item.parent.is_none()
                    || has_children[item.item_no as usize]
            })
            .map(|item| item.item_no)
            .collect();
        let placeholder = Accumulator::default().finish(completeness);
        let values = vec![placeholder; item_numbers.len()];
        let mut promoted = (completeness != ResultCompleteness::Complete && proven.is_some())
            .then(|| vec![0; items.len().div_ceil(u64::BITS as usize)]);
        if let Some(bits) = &mut promoted {
            for item in items {
                let index = item.item_no as usize;
                if crate::folder_completion::ProvenFolders::source(proven, index, completeness)
                    == ResultCompleteness::Complete
                {
                    bits[index / u64::BITS as usize] |= 1 << (index % u64::BITS as usize);
                }
            }
        }
        Self {
            item_numbers,
            values,
            source_completeness: completeness,
            promoted,
            file_gaps: Vec::new(),
        }
    }

    pub(crate) fn slot(&self, item_no: u32) -> Option<usize> {
        self.item_numbers.binary_search(&item_no).ok()
    }

    pub(crate) fn get(&self, item_no: u32) -> Option<&SpaceAggregate> {
        self.values.get(self.slot(item_no)?)
    }

    pub(crate) fn get_mut(&mut self, item_no: u32) -> Option<&mut SpaceAggregate> {
        let slot = self.slot(item_no)?;
        self.values.get_mut(slot)
    }

    pub(crate) fn source(&self, item_no: u32) -> ResultCompleteness {
        let index = item_no as usize;
        if self.promoted.as_ref().is_some_and(|bits| {
            bits.get(index / u64::BITS as usize)
                .is_some_and(|word| word & (1 << (index % u64::BITS as usize)) != 0)
        }) {
            ResultCompleteness::Complete
        } else {
            self.source_completeness
        }
    }

    pub(crate) fn mark_inaccessible(&mut self, item_no: u32, count: u64) {
        if let Some(value) = self.get_mut(item_no) {
            mark_inaccessible(value, count);
        } else {
            self.file_gaps.push((item_no, count));
        }
    }

    pub(crate) fn finish_gaps(&mut self) {
        self.file_gaps.sort_unstable_by_key(|(item_no, _)| *item_no);
    }

    pub(crate) fn item_inaccessible(&self, item_no: u32) -> bool {
        self.get(item_no)
            .is_some_and(|value| value.inaccessible_ranges.is_some_and(|count| count > 0))
            || self
                .file_gaps
                .binary_search_by_key(&item_no, |(number, _)| *number)
                .is_ok()
    }

    pub(crate) fn has_inaccessible(&self) -> bool {
        !self.file_gaps.is_empty()
            || self
                .values
                .iter()
                .any(|value| value.inaccessible_ranges.is_some_and(|count| count > 0))
    }

    pub(crate) fn file_gap_count(&self, item_no: u32) -> Option<u64> {
        self.file_gaps
            .binary_search_by_key(&item_no, |(number, _)| *number)
            .ok()
            .map(|index| self.file_gaps[index].1)
    }
}

pub(crate) fn aggregate(
    items: &[ResultItem],
    boundary: &[u32],
    completeness: ResultCompleteness,
    proven: Option<&crate::folder_completion::ProvenFolders>,
) -> SpaceAggregates {
    let mut output = SpaceAggregates::new(items, completeness, proven);
    let mut sums = vec![Accumulator::default(); output.item_numbers.len()];
    for item in items.iter().rev() {
        let mut own = Accumulator::default();
        if item.kind == ResultItemKind::UntraversedLink {
            own.observe_link(item.size.as_ref());
        } else if !is_structure(item.kind) {
            own.observe(item.size.as_ref());
        }
        // A file outside the boundary keeps its own unknown value but is an unread range
        // for its folder, so it adds neither bytes nor a missing item above it.
        if let Some(slot) = output.slot(item.item_no) {
            sums[slot].merge(&own);
            output.values[slot] = sums[slot].finish(output.source(item.item_no));
            if let Some(parent) = item.parent
                && boundary.binary_search(&item.item_no).is_err()
                && let Some(parent_slot) = output.slot(parent)
            {
                let child = sums[slot].clone();
                sums[parent_slot].merge(&child);
            }
        } else if boundary.binary_search(&item.item_no).is_err()
            && let Some(parent) = item.parent
            && let Some(parent_slot) = output.slot(parent)
        {
            sums[parent_slot].merge(&own);
        }
    }
    output
}

/// Builds the aggregate of a non-structural item from its own observation.
pub(crate) fn aggregate_item(item: &ResultItem, source: ResultCompleteness) -> SpaceAggregate {
    let mut own = Accumulator::default();
    if item.kind == ResultItemKind::UntraversedLink {
        own.observe_link(item.size.as_ref());
    } else if !is_structure(item.kind) {
        own.observe(item.size.as_ref());
    }
    own.finish(source)
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

/// Marks known unread ranges without converting unknown data into zero.
pub(crate) fn mark_inaccessible(aggregate: &mut SpaceAggregate, count: u64) {
    fn mark(value: &mut SizeValue) {
        if let SizeAmount::Exact { bytes } = &value.amount {
            value.amount = if bytes.get() == 0 {
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
    mark(&mut aggregate.logical);
    mark(&mut aggregate.allocated);
    mark(&mut aggregate.unique_physical);
    state(&mut aggregate.completeness);
    state(&mut aggregate.allocated_completeness);
    state(&mut aggregate.unique_physical_completeness);
    aggregate.inaccessible_ranges = Some(count);
}
