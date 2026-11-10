//! Table-driven F03 volume gap arithmetic over observations from one pinned result.

use du_core_contracts::{ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue};
use std::collections::BTreeMap;

/// Independent contributions to the gap between volume use and scoped allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Factor {
    OutOfScope,
    Excluded,
    Inaccessible,
    Dedup,
    AuxNotCollected,
    FilesystemMetadata,
    OsCategory,
    TimeSkew,
}

const FACTORS: [(Factor, &str); 8] = [
    (Factor::OutOfScope, "gap.out_of_scope"),
    (Factor::Excluded, "gap.excluded"),
    (Factor::Inaccessible, "gap.inaccessible"),
    (Factor::Dedup, "gap.dedup"),
    (Factor::AuxNotCollected, "gap.aux_not_collected"),
    (Factor::FilesystemMetadata, "gap.fs_metadata"),
    (Factor::OsCategory, "gap.os_category"),
    (Factor::TimeSkew, "gap.time_skew"),
];

/// Exact attributed bytes and their evidence, or the reason attribution is unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FactorValue {
    Exact { bytes: u64, evidence: SizeEvidence },
    Unknown { reason_key: String },
}

impl FactorValue {
    fn insufficient() -> Self {
        Self::Unknown {
            reason_key: "space.data_insufficient".into(),
        }
    }

    fn exact_bytes(&self) -> Option<u64> {
        match self {
            Self::Exact { bytes, evidence } if *evidence != SizeEvidence::Unknown => Some(*bytes),
            _ => None,
        }
    }
}

/// One factor in the published order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactorRow {
    pub key: &'static str,
    pub value: FactorValue,
}

/// A reason the unknown remainder cannot be quantified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemainderUnavailable {
    VolumeReportUnavailable,
    InexactInput,
    NegativeDifference,
}

/// The factor table and the unaccounted bytes, if every operand is exact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeGap {
    pub factors: [FactorRow; FACTORS.len()],
    pub remainder: Result<ByteSize, RemainderUnavailable>,
}

impl VolumeGap {
    /// Subtracts exact allocated bytes and independently attributed factors from
    /// reported used bytes. The caller supplies evidence from the same pinned
    /// volume and scope. Omitted factors remain unknown.
    pub fn calculate(
        used_bytes: Option<u64>,
        scoped: &SizeValue,
        observed: &BTreeMap<Factor, FactorValue>,
    ) -> Self {
        let factors = FACTORS.map(|(factor, key)| {
            let value = match observed.get(&factor) {
                Some(value @ FactorValue::Unknown { .. }) => value.clone(),
                Some(value) if value.exact_bytes().is_some() => value.clone(),
                _ => FactorValue::insufficient(),
            };
            FactorRow { key, value }
        });
        Self {
            remainder: remainder(used_bytes, scoped, &factors),
            factors,
        }
    }
}

fn remainder(
    used_bytes: Option<u64>,
    scoped: &SizeValue,
    factors: &[FactorRow; FACTORS.len()],
) -> Result<ByteSize, RemainderUnavailable> {
    use RemainderUnavailable::{InexactInput, NegativeDifference, VolumeReportUnavailable};

    let used_bytes = used_bytes.ok_or(VolumeReportUnavailable)?;
    let SizeAmount::Exact { bytes } = &scoped.amount else {
        return Err(InexactInput);
    };
    if scoped.basis != SizeBasis::Allocated || scoped.evidence == SizeEvidence::Unknown {
        return Err(InexactInput);
    }
    let mut known = [0; FACTORS.len()];
    for (bytes, row) in known.iter_mut().zip(factors) {
        *bytes = row.value.exact_bytes().ok_or(InexactInput)?;
    }
    let allocated = bytes.as_str().parse::<u64>().map_err(|_| InexactInput)?;
    std::iter::once(allocated)
        .chain(known)
        .try_fold(used_bytes, u64::checked_sub)
        .map(ByteSize::from)
        .ok_or(NegativeDifference)
}
