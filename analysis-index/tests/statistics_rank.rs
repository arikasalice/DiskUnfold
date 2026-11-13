use du_analysis_index::{
    IngestSink, Observation,
    statistics::{self, Candidate, Level},
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn size(amount: SizeAmount) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount,
    }
}
fn candidates(version: &du_analysis_index::ResultVersion) -> Vec<Candidate> {
    version
        .items()
        .iter()
        .filter(|i| i.parent == Some(0))
        .map(|i| Candidate {
            item: ItemRef {
                result_version: version.version_id().clone(),
                item_no: i.item_no,
            },
            path_compare_key: i.name.clone(),
        })
        .collect()
}

#[test]
fn ranks_ties_lower_bounds_unknown_and_zero_without_mixing_levels() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![b"folder".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    for (name, value) in [
        (
            "z",
            Some(size(SizeAmount::Exact {
                bytes: 10_u64.into(),
            })),
        ),
        (
            "a",
            Some(size(SizeAmount::LowerBound {
                bytes: 10_u64.into(),
            })),
        ),
        (
            "zero",
            Some(size(SizeAmount::Exact {
                bytes: 0_u64.into(),
            })),
        ),
        ("unknown", None),
    ] {
        b.add_file(Observation {
            path: vec![name.as_bytes().to_vec()],
            size: value,
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    let v = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let result = statistics::rank_size(
        &v,
        SizeBasis::Logical,
        Level::File,
        10,
        &candidates(&v),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let names: Vec<_> = result
        .ranked
        .iter()
        .map(|r| v.items()[r.item.item_no as usize].name.as_slice())
        .collect();
    assert_eq!(names, vec![b"a".as_slice(), b"z", b"zero"]);
    assert!(matches!(
        result.ranked[0].size.amount,
        SizeAmount::LowerBound { .. }
    ));
    assert_eq!(
        result.ranked.iter().map(|r| r.position).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        v.items()[result.unknown[0].item_no as usize].name,
        b"unknown"
    );
    assert!(result.has_sizes);
    let mut reversed = candidates(&v);
    reversed.reverse();
    let top = statistics::rank_size(
        &v,
        SizeBasis::Logical,
        Level::File,
        1,
        &reversed,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(top.ranked, result.ranked[..1]);
    assert_eq!(top.unknown, result.unknown);
    let limited = statistics::rank_size(
        &v,
        SizeBasis::Logical,
        Level::File,
        0,
        &candidates(&v),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(limited.ranked.is_empty() && limited.has_sizes);
    let empty = statistics::rank_size(&v, SizeBasis::Logical, Level::File, 3, &[], &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(!empty.has_sizes && empty.ranked.is_empty() && empty.unknown.is_empty());
    let allocated = statistics::rank_size(
        &v,
        SizeBasis::Allocated,
        Level::File,
        3,
        &candidates(&v),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(!allocated.has_sizes && allocated.unknown.len() == 4);
    let folders = statistics::rank_size(
        &v,
        SizeBasis::Logical,
        Level::Directory,
        3,
        &candidates(&v),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(folders.ranked.len(), 1);
    assert_eq!(
        v.items()[folders.ranked[0].item.item_no as usize].name,
        b"folder"
    );
    let mut duplicate_keys = candidates(&v);
    duplicate_keys[1].path_compare_key = duplicate_keys[0].path_compare_key.clone();
    assert_eq!(
        statistics::rank_size(
            &v,
            SizeBasis::Logical,
            Level::File,
            3,
            &duplicate_keys,
            &mut || false
        )
        .err(),
        Some(statistics::Error::InvalidSelection)
    );
    Ok(())
}

#[test]
fn pinned_old_version_stays_readable_and_mixed_versions_fail() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    b.add_file(Observation {
        path: vec![b"x".to_vec()],
        size: Some(size(SizeAmount::Exact {
            bytes: 1_u64.into(),
        })),
    })
    .map_err(|e| format!("{e:?}"))?;
    let old = b.publish_partial(id(4)?).map_err(|e| format!("{e:?}"))?;
    let old_candidates = candidates(&old);
    let new = b
        .seal(id(5)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let rank = statistics::rank_size(
        &old,
        SizeBasis::Logical,
        Level::File,
        5,
        &old_candidates,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(rank.result_version, *old.version_id());
    assert_eq!(
        statistics::rank_size(
            &new,
            SizeBasis::Logical,
            Level::File,
            5,
            &old_candidates,
            &mut || false
        )
        .err(),
        Some(statistics::Error::VersionMismatch)
    );
    assert_eq!(
        statistics::rank_size(
            &old,
            SizeBasis::Logical,
            Level::File,
            5,
            &old_candidates,
            &mut || true
        )
        .err(),
        Some(statistics::Error::Cancelled)
    );
    Ok(())
}
