//! Crate-internal checks of how a build shares file space evidence.
use crate::{FileIdentity, IngestSink, Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue};
use std::sync::Arc;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).map_err(|e| format!("{e:?}"))
}

#[test]
fn two_partial_versions_of_one_build_share_each_files_evidence() -> Result<(), String> {
    let failed = |e: crate::BuildError| format!("{e:?}");
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(failed)?;
    build.add_directory(vec![]).map_err(failed)?;
    let space = SpaceObservation {
        allocated: Some(SizeValue {
            basis: SizeBasis::Allocated,
            amount: SizeAmount::Exact { bytes: 4096.into() },
            evidence: SizeEvidence::OsReported,
        }),
        identity: Some(FileIdentity {
            volume: vec![1],
            file: vec![7],
        }),
        link_count: Some(1),
        sharing: SharingEvidence::None,
    };
    let file = Observation {
        path: vec![b"file".to_vec()],
        size: None,
    };
    build.add_with_space(file, space).map_err(failed)?;
    let first = build.publish_partial(id(2)?).map_err(failed)?;
    let second = build.publish_partial(id(3)?).map_err(failed)?;
    let find = |version: &crate::ResultVersion| {
        version
            .physical
            .as_ref()
            .and_then(|values| values.iter().flatten().next().cloned())
    };
    let (Some(a), Some(b)) = (find(&first), find(&second)) else {
        return Err("both versions must keep the file evidence".into());
    };
    assert!(Arc::ptr_eq(&a, &b));
    Ok(())
}
