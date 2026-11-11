use du_analysis_index::volume_gap::{
    Factor, FactorValue, GapRemainder, RemainderUnavailable, VolumeGap,
};
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, SizeValue};
use std::collections::BTreeMap;

fn allocated(bytes: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Allocated,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}

fn exact(bytes: u64) -> FactorValue {
    FactorValue::Exact {
        bytes,
        evidence: SizeEvidence::OsReported,
    }
}

fn observed() -> BTreeMap<Factor, FactorValue> {
    use Factor::*;
    [
        (TimeSkew, 0),
        (OsCategory, 4),
        (FilesystemMetadata, 3),
        (AuxNotCollected, 0),
        (Dedup, 2),
        (Inaccessible, 0),
        (Excluded, 1),
        (OutOfScope, 10),
    ]
    .into_iter()
    .map(|(factor, bytes)| (factor, exact(bytes)))
    .collect()
}

#[test]
fn preserves_factor_evidence_and_calculates_in_published_order() {
    let mut factors = observed();
    factors.insert(
        Factor::Excluded,
        FactorValue::Exact {
            bytes: 1,
            evidence: SizeEvidence::Estimated,
        },
    );
    let gap = VolumeGap::calculate(Some(100), &allocated(75), &factors);
    assert_eq!(
        gap.remainder,
        Ok(GapRemainder {
            bytes: 7.into(),
            evidence: SizeEvidence::Estimated
        })
    );
    assert_eq!(
        gap.factors.each_ref().map(|row| row.key),
        [
            "gap.out_of_scope",
            "gap.excluded",
            "gap.inaccessible",
            "gap.dedup",
            "gap.aux_not_collected",
            "gap.fs_metadata",
            "gap.os_category",
            "gap.time_skew",
        ]
    );
    assert_eq!(gap.factors[2].value, exact(0));
    assert_eq!(gap.factors[6].value, exact(4));
}

#[test]
fn dedup_explains_scoped_allocation_without_changing_the_remainder() {
    let mut factors = observed();
    let dedup = FactorValue::Exact {
        bytes: 90,
        evidence: SizeEvidence::Estimated,
    };
    factors.insert(Factor::Dedup, dedup.clone());
    let gap = VolumeGap::calculate(Some(100), &allocated(75), &factors);
    assert_eq!(gap.factors[3].value, dedup);
    assert_eq!(
        gap.remainder,
        Ok(GapRemainder {
            bytes: 7.into(),
            evidence: SizeEvidence::OsReported,
        })
    );
}

#[test]
fn unobserved_factors_and_inexact_allocation_never_become_zero() {
    let mut factors = observed();
    let explicit_unknown = FactorValue::Unknown {
        reason_key: "space.aux_not_included".into(),
    };
    factors.insert(Factor::AuxNotCollected, explicit_unknown.clone());
    factors.remove(&Factor::Inaccessible);
    factors.insert(
        Factor::TimeSkew,
        FactorValue::Exact {
            bytes: 0,
            evidence: SizeEvidence::Unknown,
        },
    );
    let gap = VolumeGap::calculate(Some(100), &allocated(75), &factors);
    assert_eq!(gap.remainder, Err(RemainderUnavailable::InexactInput));
    assert_eq!(gap.factors[4].value, explicit_unknown);
    let absent = FactorValue::Unknown {
        reason_key: "space.data_insufficient".into(),
    };
    assert_eq!(gap.factors[2].value, absent);
    assert_eq!(gap.factors[7].value, absent);
    let mut missing_dedup = observed();
    missing_dedup.remove(&Factor::Dedup);
    let gap = VolumeGap::calculate(Some(100), &allocated(75), &missing_dedup);
    assert_eq!(gap.factors[3].value, absent);
    assert_eq!(gap.remainder, Err(RemainderUnavailable::InexactInput));
    let lower_bound = SizeValue {
        amount: SizeAmount::LowerBound { bytes: 75.into() },
        ..allocated(75)
    };
    let logical = SizeValue {
        basis: SizeBasis::Logical,
        ..allocated(75)
    };
    for invalid in [lower_bound, logical] {
        assert_eq!(
            VolumeGap::calculate(Some(100), &invalid, &observed()).remainder,
            Err(RemainderUnavailable::InexactInput)
        );
    }
}

#[test]
fn exact_zero_missing_report_and_negative_difference_are_distinct() {
    let mut factors = observed();
    assert_eq!(
        VolumeGap::calculate(Some(93), &allocated(75), &factors).remainder,
        Ok(GapRemainder {
            bytes: 0.into(),
            evidence: SizeEvidence::OsReported
        })
    );
    assert_eq!(
        VolumeGap::calculate(None, &allocated(75), &factors).remainder,
        Err(RemainderUnavailable::VolumeReportUnavailable)
    );
    assert_eq!(
        VolumeGap::calculate(Some(92), &allocated(75), &factors).remainder,
        Err(RemainderUnavailable::NegativeDifference)
    );
    factors.insert(Factor::OutOfScope, exact(u64::MAX));
    assert_eq!(
        VolumeGap::calculate(Some(u64::MAX), &allocated(0), &factors).remainder,
        Err(RemainderUnavailable::NegativeDifference)
    );
}
