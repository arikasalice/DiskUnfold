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

/// Exact unaccounted bytes and the weakest contributing evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapRemainder {
    pub bytes: ByteSize,
    pub evidence: SizeEvidence,
}

/// The factor table and the unaccounted bytes, if every operand is exact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeGap {
    pub factors: [FactorRow; FACTORS.len()],
    pub remainder: Result<GapRemainder, RemainderUnavailable>,
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
) -> Result<GapRemainder, RemainderUnavailable> {
    use RemainderUnavailable::{InexactInput, NegativeDifference, VolumeReportUnavailable};

    let used_bytes = used_bytes.ok_or(VolumeReportUnavailable)?;
    let SizeAmount::Exact { bytes } = &scoped.amount else {
        return Err(InexactInput);
    };
    if scoped.basis != SizeBasis::Allocated || scoped.evidence == SizeEvidence::Unknown {
        return Err(InexactInput);
    }
    let mut known = [0; FACTORS.len()];
    let mut evidence =
        crate::space::weaker(Some(SizeEvidence::OsReported), scoped.evidence.clone());
    for (bytes, row) in known.iter_mut().zip(factors) {
        let FactorValue::Exact {
            bytes: value,
            evidence: source,
        } = &row.value
        else {
            return Err(InexactInput);
        };
        *bytes = *value;
        evidence = crate::space::weaker(Some(evidence), source.clone());
    }
    let allocated = bytes.as_str().parse::<u64>().map_err(|_| InexactInput)?;
    std::iter::once(allocated)
        .chain(known)
        .try_fold(used_bytes, u64::checked_sub)
        .map(|bytes| GapRemainder {
            bytes: bytes.into(),
            evidence,
        })
        .ok_or(NegativeDifference)
}

/// Selects exactly one actual root or every root of one known volume.
pub enum VolumeGapTarget<'a> {
    Root(&'a du_core_contracts::ItemRef),
    Volume(&'a [u8]),
}

/// Volume observations and scoped physical totals from a single pinned result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedVolumeGap {
    pub result_version: du_core_contracts::Id,
    /// None means the selected root's volume identity was not established.
    pub volume_id: Option<Vec<u8>>,
    pub start: Option<crate::VolumeReportPoint>,
    pub end: Option<crate::VolumeReportPoint>,
    /// Reason when the required ending volume observation is unavailable.
    pub volume_report_unavailable_reason: Option<&'static str>,
    pub allocated: SizeValue,
    /// Completeness of allocated observations in the selected roots.
    pub allocated_completeness: du_core_contracts::result::ResultCompleteness,
    pub unique_physical: SizeValue,
    /// Completeness of unique physical evidence in the selected roots.
    pub unique_physical_completeness: du_core_contracts::result::ResultCompleteness,
    pub gap: VolumeGap,
}

impl crate::ResultVersion {
    /// Computes evidence only from this result; no filesystem access is performed.
    /// A root reference must name an actual directory root in this same version.
    pub fn volume_gap(
        &self,
        target: VolumeGapTarget<'_>,
    ) -> Result<PinnedVolumeGap, crate::QueryError> {
        use du_core_contracts::result::{ResultCompleteness, ResultItemKind};
        let roots = match target {
            VolumeGapTarget::Root(reference) => {
                let item = self
                    .detail(reference)
                    .map_err(|error| match error {
                        crate::QueryError::NotFound => crate::QueryError::InvalidPath,
                        other => other,
                    })?
                    .item;
                if item.parent.is_some() || item.kind != ResultItemKind::Directory {
                    return Err(crate::QueryError::InvalidPath);
                }
                let (index, _) = self.observed_root_for(reference)?;
                vec![index]
            }
            VolumeGapTarget::Volume(volume) => {
                if volume.is_empty() {
                    return Err(crate::QueryError::InvalidPath);
                }
                let roots: Vec<_> = self
                    .roots
                    .iter()
                    .enumerate()
                    .filter(|(_, root)| root.volume_id == volume)
                    .map(|(index, _)| index as u32)
                    .collect();
                if roots.is_empty() {
                    return Err(crate::QueryError::InvalidPath);
                }
                roots
            }
        };
        let identity = self
            .roots
            .get(roots[0] as usize)
            .ok_or(crate::QueryError::PathUnavailable)?
            .volume_id
            .clone();
        let volume_id = (!identity.is_empty()).then_some(identity);
        let report = volume_id.as_ref().and_then(|id| {
            self.volume_reports
                .iter()
                .find(|report| report.volume_id == *id)
        });
        let start = report.and_then(|report| report.start.clone());
        let end = report.and_then(|report| report.end.clone());
        let used = end
            .as_ref()
            .and_then(|point| point.value.as_ref().ok())
            .map(|value| value.used_bytes);
        let has_gap = self
            .gaps
            .iter()
            .any(|gap| roots.binary_search(&gap.root_index).is_ok());
        let unknown_structure = self.items.iter().enumerate().any(|(index, item)| {
            roots.binary_search(&self.root_indices[index]).is_ok()
                && item.kind == ResultItemKind::UnknownStructure
        });
        let completeness = match self.completeness {
            ResultCompleteness::Partial => ResultCompleteness::Partial,
            ResultCompleteness::Cancelled => ResultCompleteness::Cancelled,
            ResultCompleteness::Missing if self.gaps.is_empty() => ResultCompleteness::Missing,
            _ if has_gap || unknown_structure => ResultCompleteness::Missing,
            _ => ResultCompleteness::Complete,
        };
        let mut aggregate = self
            .aggregates
            .get(roots[0] as usize)
            .ok_or(crate::QueryError::PathUnavailable)?
            .clone();
        crate::physical::selected_scope(self, &roots, completeness, &mut aggregate);
        let gap = VolumeGap::calculate(used, &aggregate.allocated, &BTreeMap::new());
        Ok(PinnedVolumeGap {
            result_version: self.version_id.clone(),
            volume_id,
            start,
            volume_report_unavailable_reason: used
                .is_none()
                .then_some("space.volume_report_unavailable"),
            end,
            allocated: aggregate.allocated,
            allocated_completeness: aggregate.allocated_completeness,
            unique_physical: aggregate.unique_physical,
            unique_physical_completeness: aggregate.unique_physical_completeness,
            gap,
        })
    }
}
