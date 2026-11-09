use du_core_contracts::result::{ResultCompleteness, ResultItem, SpaceAggregate};
use du_core_contracts::{ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue};

#[derive(Clone, Default)]
struct Accumulator {
    bytes: Option<u64>,
    missing: u64,
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
        // ByteSize validates the u64 range at construction and deserialization.
        if let Ok(bytes) = bytes.as_str().parse::<u64>() {
            self.merge_bytes(Some(bytes));
            self.lower_bound |= lower;
            self.evidence = Some(weaker(self.evidence.take(), value.evidence.clone()));
        } else {
            self.missing += 1;
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
                if self.missing > 0 || self.overflow || self.lower_bound =>
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

fn weaker(left: Option<SizeEvidence>, right: SizeEvidence) -> SizeEvidence {
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
    structures: &[bool],
    completeness: ResultCompleteness,
) -> Vec<SpaceAggregate> {
    let mut sums = vec![Accumulator::default(); items.len()];
    for item in items.iter().rev() {
        let index = item.item_no as usize;
        if !structures[index] {
            sums[index].observe(item.size.as_ref());
        }
        if let Some(parent) = item.parent {
            let child = sums[index].clone();
            sums[parent as usize].merge(&child);
        }
    }
    sums.iter().map(|sum| sum.finish(completeness)).collect()
}
