//! #149, F06 README 28: an unavailable answer names the size basis as its cause
//! only when the result shows it. The scope has no unique physical size because
//! sharing evidence was not collected, while its allocated size is known, so
//! another basis has values. Otherwise the reason stays as the table decided.
use crate::required_data::Reason;
use du_analysis_index::ResultVersion;
use du_core_contracts::{ItemRef, SizeAmount, SizeBasis};

const NOT_COLLECTED: &str = "space.sharing_not_collected";

pub(super) fn refine(
    reason: Reason,
    version: &ResultVersion,
    scope: &ItemRef,
    basis: &SizeBasis,
) -> Reason {
    if reason != Reason::NoItemSizes || *basis != SizeBasis::UniquePhysical {
        return reason;
    }
    let Some(space) = version.space(scope.item_no) else {
        return reason;
    };
    let known = |amount: &SizeAmount| !matches!(amount, SizeAmount::Unknown);
    if !known(&space.unique_physical.amount)
        && space.limitations.contains(&NOT_COLLECTED)
        && known(&space.allocated.amount)
    {
        Reason::BasisNotCounted
    } else {
        reason
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_analysis_index::{
        FileIdentity, IngestSink, Observation, SharingEvidence, SpaceObservation,
    };
    use du_core_contracts::{ByteSize, Id, SizeEvidence, SizeValue, result::ResultCompleteness};
    use std::sync::Arc;

    fn id(n: u8) -> Result<Id, String> {
        Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
    }

    fn size(basis: SizeBasis, bytes: u64) -> SizeValue {
        SizeValue {
            basis,
            evidence: SizeEvidence::OsReported,
            amount: SizeAmount::Exact {
                bytes: ByteSize::from(bytes),
            },
        }
    }

    /// A scope of one identified file with unknown sharing, with or without an
    /// allocated size.
    fn scope(allocated: Option<u64>) -> Result<(Arc<ResultVersion>, ItemRef), String> {
        let sink = IngestSink::default();
        let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
        build
            .add_directory(Vec::new())
            .map_err(|e| format!("{e:?}"))?;
        let observation = Observation {
            path: vec![b"a".to_vec()],
            size: Some(size(SizeBasis::Logical, 5)),
        };
        let space = SpaceObservation {
            allocated: allocated.map(|bytes| size(SizeBasis::Allocated, bytes)),
            identity: Some(FileIdentity {
                volume: vec![1],
                file: vec![1],
            }),
            link_count: Some(1),
            sharing: SharingEvidence::Unknown,
        };
        build
            .add_with_space_at(0, observation, space)
            .map_err(|e| format!("{e:?}"))?;
        let version = build
            .seal(id(2)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        let scope = ItemRef {
            result_version: id(2)?,
            item_no: 0,
        };
        Ok((version, scope))
    }

    #[test]
    fn uncollected_sharing_with_an_allocated_size_names_the_basis() -> Result<(), String> {
        let (version, scope) = scope(Some(4096))?;
        let space = version.space(0).ok_or("no aggregate")?;
        assert!(space.limitations.contains(&NOT_COLLECTED));
        let unique = SizeBasis::UniquePhysical;
        assert_eq!(
            refine(Reason::NoItemSizes, &version, &scope, &unique),
            Reason::BasisNotCounted
        );
        // Another basis, or another reason, is left as the table decided.
        assert_eq!(
            refine(Reason::NoItemSizes, &version, &scope, &SizeBasis::Allocated),
            Reason::NoItemSizes
        );
        assert_eq!(
            refine(Reason::NoAttribution, &version, &scope, &unique),
            Reason::NoAttribution
        );
        Ok(())
    }

    #[test]
    fn without_an_allocated_size_the_cause_is_not_the_basis() -> Result<(), String> {
        let (version, scope) = scope(None)?;
        assert_eq!(
            refine(
                Reason::NoItemSizes,
                &version,
                &scope,
                &SizeBasis::UniquePhysical
            ),
            Reason::NoItemSizes
        );
        Ok(())
    }
}
