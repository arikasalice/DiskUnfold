use du_analysis_index::{
    FileIdentity, IngestSink, Observation, QueryError, SharingEvidence, SpaceObservation,
    space_detail::ExclusiveDecision,
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn root(volume: u8, name: &str) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(name.as_bytes()),
        },
        volume_id: vec![volume],
        file_id: vec![1],
    }
}
fn value(basis: SizeBasis, n: u64) -> SizeValue {
    SizeValue {
        basis: basis.clone(),
        evidence: if basis == SizeBasis::Logical {
            SizeEvidence::Measured
        } else {
            SizeEvidence::OsReported
        },
        amount: SizeAmount::Exact { bytes: n.into() },
    }
}
fn add(
    build: &mut du_analysis_index::BuildHandle,
    root: u32,
    name: &str,
    volume: u8,
    file: u8,
    count: Option<u64>,
    sharing: SharingEvidence,
) -> Result<(), String> {
    build
        .add_with_space_at(
            root,
            Observation {
                path: vec![name.as_bytes().to_vec()],
                size: Some(value(SizeBasis::Logical, 11)),
            },
            SpaceObservation {
                allocated: Some(value(SizeBasis::Allocated, 8)),
                identity: Some(FileIdentity {
                    volume: vec![volume],
                    file: vec![file],
                }),
                link_count: count,
                sharing,
            },
        )
        .map_err(|e| format!("{e:?}"))
}
fn reference(result: &du_analysis_index::ResultVersion, name: &str) -> Result<ItemRef, String> {
    let item = result
        .items()
        .iter()
        .find(|item| item.name == name.as_bytes())
        .ok_or("missing item")?;
    Ok(ItemRef {
        result_version: result.version_id().clone(),
        item_no: item.item_no,
    })
}
#[test]
fn links_only_match_same_volume_and_preserve_exclusivity_evidence() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root(1, "a"), root(1, "b"), root(2, "c")])
        .map_err(|e| format!("{e:?}"))?;
    for i in 0..3 {
        build
            .add_directory_at(i, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    add(&mut build, 0, "one", 1, 7, Some(2), SharingEvidence::None)?;
    add(&mut build, 1, "two", 1, 7, Some(2), SharingEvidence::None)?;
    add(
        &mut build,
        2,
        "different",
        2,
        7,
        Some(1),
        SharingEvidence::Unknown,
    )?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let detail = result
        .space_detail(&reference(&result, "one")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(detail.logical, Some(value(SizeBasis::Logical, 11)));
    assert_eq!(detail.allocated, Some(value(SizeBasis::Allocated, 8)));
    assert!(detail.unique_physical.is_none());
    let links = detail.links.ok_or("missing links")?;
    assert_eq!(links.in_scope.ok_or("missing group")?.len(), 2);
    assert_eq!(links.observed_link_count, Some(2));
    assert_eq!(links.outside_scope, Some(false));
    assert_eq!(detail.exclusive, ExclusiveDecision::Confirmed);
    let different = result
        .space_detail(&reference(&result, "different")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        different
            .links
            .ok_or("missing")?
            .in_scope
            .ok_or("missing")?
            .len(),
        1
    );
    assert_eq!(
        different.exclusive,
        ExclusiveDecision::Unknown {
            reason_key: "space.sharing_unknown"
        }
    );
    Ok(())
}
#[test]
fn inconsistent_link_counts_and_incomplete_scope_are_never_confirmed() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root(1, "a"), root(1, "b")])
        .map_err(|e| format!("{e:?}"))?;
    for i in 0..2 {
        build
            .add_directory_at(i, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    add(&mut build, 0, "one", 1, 7, Some(2), SharingEvidence::None)?;
    add(&mut build, 1, "two", 1, 7, Some(3), SharingEvidence::None)?;
    add(
        &mut build,
        0,
        "outside",
        1,
        9,
        Some(2),
        SharingEvidence::None,
    )?;
    let partial = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    let p = partial
        .space_detail(&reference(&partial, "outside")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(p.links.ok_or("missing")?.outside_scope, None);
    assert_eq!(
        p.exclusive,
        ExclusiveDecision::Unknown {
            reason_key: "space.data_insufficient"
        }
    );
    let complete = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let conflict = complete
        .space_detail(&reference(&complete, "one")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        conflict.links.ok_or("missing")?.counts_consistent,
        Some(false)
    );
    assert_eq!(
        conflict.exclusive,
        ExclusiveDecision::Unknown {
            reason_key: "space.data_insufficient"
        }
    );
    let outside = complete
        .space_detail(&reference(&complete, "outside")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(outside.links.ok_or("missing")?.outside_scope, Some(true));
    assert_eq!(
        outside.exclusive,
        ExclusiveDecision::Unknown {
            reason_key: "space.out_of_range_links"
        }
    );
    Ok(())
}
#[test]
fn folder_and_wrong_result_are_distinct() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root(1, "a")])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let item = ItemRef {
        result_version: id(2)?,
        item_no: 0,
    };
    let detail = result.space_detail(&item).map_err(|e| format!("{e:?}"))?;
    assert!(detail.logical.is_some());
    assert!(detail.links.is_none());
    assert_eq!(detail.exclusive, ExclusiveDecision::NotApplicable);
    assert!(matches!(
        result.space_detail(&ItemRef {
            result_version: id(3)?,
            item_no: 0
        }),
        Err(QueryError::StaleVersion)
    ));
    Ok(())
}

#[test]
fn unavailable_link_count_is_not_a_contradiction_or_proof() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root(1, "a"), root(1, "b")])
        .map_err(|e| format!("{e:?}"))?;
    for i in 0..2 {
        build
            .add_directory_at(i, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    add(&mut build, 0, "known", 1, 7, Some(2), SharingEvidence::None)?;
    add(&mut build, 1, "unknown", 1, 7, None, SharingEvidence::None)?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let detail = result
        .space_detail(&reference(&result, "known")?)
        .map_err(|e| format!("{e:?}"))?;
    let links = detail.links.ok_or("missing links")?;
    assert_eq!(links.observed_link_count, Some(2));
    assert_eq!(links.counts_consistent, None);
    assert_eq!(links.outside_scope, None);
    assert_eq!(
        detail.exclusive,
        ExclusiveDecision::Unknown {
            reason_key: "space.data_insufficient"
        }
    );
    Ok(())
}

#[test]
fn missing_allocation_and_unverified_identity_follow_first_match() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root(1, "a")])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_file_at(
            0,
            Observation {
                path: vec![b"unallocated".to_vec()],
                size: Some(value(SizeBasis::Logical, 11)),
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_with_space_at(
            0,
            Observation {
                path: vec![b"no_identity".to_vec()],
                size: Some(value(SizeBasis::Logical, 11)),
            },
            SpaceObservation {
                allocated: Some(value(SizeBasis::Allocated, 8)),
                identity: None,
                link_count: None,
                sharing: SharingEvidence::Unknown,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let missing = result
        .space_detail(&reference(&result, "unallocated")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        missing.exclusive,
        ExclusiveDecision::Unknown {
            reason_key: "space.data_insufficient"
        }
    );
    let identity = result
        .space_detail(&reference(&result, "no_identity")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        identity.exclusive,
        ExclusiveDecision::Unknown {
            reason_key: "space.identity_unsupported"
        }
    );
    Ok(())
}

#[test]
fn unavailable_shared_observation_does_not_confirm_exclusive() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root(1, "a"), root(1, "b")])
        .map_err(|e| format!("{e:?}"))?;
    for i in 0..2 {
        build
            .add_directory_at(i, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    add(&mut build, 0, "first", 1, 7, Some(2), SharingEvidence::None)?;
    add(
        &mut build,
        1,
        "second",
        1,
        7,
        Some(2),
        SharingEvidence::Unknown,
    )?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let detail = result
        .space_detail(&reference(&result, "first")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(detail.links.ok_or("missing")?.facts_consistent, None);
    assert_eq!(
        detail.exclusive,
        ExclusiveDecision::Unknown {
            reason_key: "space.data_insufficient"
        }
    );
    Ok(())
}
