//! Final aggregate fields from an accumulated identity table.

use super::*;

impl Physical {
    pub(super) fn finish(&self, source: ResultCompleteness, output: &mut SpaceAggregate) {
        let overflow = self.total > u128::from(u64::MAX);
        let sharing_unknown = self.sharing_unknown > 0;
        let lower = self.lower > 0;
        let mut missing = self.missing || self.conflicts > 0;
        let bytes = if self.known > 0 && !overflow {
            u64::try_from(self.total).ok()
        } else if self.known == 0 && !missing && source == ResultCompleteness::Complete {
            Some(0)
        } else {
            None
        };
        let capacity_exceeded = self.capacity_exceeded();
        missing |= bytes.is_none() || capacity_exceeded;
        let completeness = state(source, missing || lower || overflow);
        output.allocated_completeness = completeness;
        output.allocated = amount(
            SizeBasis::Allocated,
            bytes,
            completeness,
            overflow || capacity_exceeded,
        );
        output.unique_physical_completeness = state(completeness, sharing_unknown);
        output.unique_physical = if sharing_unknown || missing {
            amount(
                SizeBasis::UniquePhysical,
                None,
                output.unique_physical_completeness,
                false,
            )
        } else {
            amount(
                SizeBasis::UniquePhysical,
                bytes,
                output.unique_physical_completeness,
                overflow,
            )
        };
        if self.known == 0 && !missing && bytes == Some(0) {
            output.allocated.evidence = SizeEvidence::Measured;
            output.unique_physical.evidence = SizeEvidence::Measured;
        }
        self.set_limitations(
            output,
            !missing && bytes.is_some(),
            sharing_unknown,
            overflow,
        );
    }

    fn set_limitations(
        &self,
        output: &mut SpaceAggregate,
        has_data: bool,
        sharing_unknown: bool,
        overflow: bool,
    ) {
        if has_data {
            const KNOWN: &[&str] = &[
                "space.aux_not_included",
                "space.observation_detail_insufficient",
            ];
            const KNOWN_OVERFLOW: &[&str] = &[
                "space.aux_not_included",
                "space.observation_detail_insufficient",
                "space.aggregation_overflow",
            ];
            const SHARING_UNKNOWN: &[&str] = &[
                "space.sharing_not_collected",
                "space.aux_not_included",
                "space.observation_detail_insufficient",
            ];
            const SHARING_UNKNOWN_OVERFLOW: &[&str] = &[
                "space.sharing_not_collected",
                "space.aux_not_included",
                "space.observation_detail_insufficient",
                "space.aggregation_overflow",
            ];
            let logical_overflow = output.limitations.contains(&"space.aggregation_overflow");
            output.limitations =
                std::borrow::Cow::Borrowed(match (sharing_unknown, logical_overflow) {
                    (false, false) => KNOWN,
                    (false, true) => KNOWN_OVERFLOW,
                    (true, false) => SHARING_UNKNOWN,
                    (true, true) => SHARING_UNKNOWN_OVERFLOW,
                });
        }
        if self.identity_missing {
            output
                .limitations
                .to_mut()
                .push("space.hardlink_identity_unknown");
        }
        if self.capacity_exceeded() {
            output.limitations.to_mut().push("space.dedup_capacity");
        }
        if overflow && !output.limitations.contains(&"space.aggregation_overflow") {
            output
                .limitations
                .to_mut()
                .push("space.aggregation_overflow");
        }
    }
}

pub(super) fn state(source: ResultCompleteness, missing: bool) -> ResultCompleteness {
    match source {
        ResultCompleteness::Complete if missing => ResultCompleteness::Missing,
        other => other,
    }
}

pub(super) fn amount(
    basis: SizeBasis,
    bytes: Option<u64>,
    completeness: ResultCompleteness,
    overflow: bool,
) -> SizeValue {
    let amount = match (bytes, completeness, overflow) {
        (_, _, true) | (None, _, _) => SizeAmount::Unknown,
        (Some(bytes), ResultCompleteness::Complete, false) => SizeAmount::Exact {
            bytes: ByteSize::from(bytes),
        },
        (Some(bytes), _, false) => SizeAmount::LowerBound {
            bytes: ByteSize::from(bytes),
        },
    };
    let evidence = if matches!(amount, SizeAmount::Unknown) {
        SizeEvidence::Unknown
    } else {
        SizeEvidence::OsReported
    };
    SizeValue {
        basis,
        amount,
        evidence,
    }
}
