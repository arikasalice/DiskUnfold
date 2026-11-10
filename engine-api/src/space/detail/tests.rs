use super::*;
use du_analysis_index::{Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn root(name: &[u8], volume: u8) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(name),
        },
        volume_id: vec![volume],
        file_id: vec![1],
    }
}

fn logical(bytes: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}

fn reference(version: &Id, item_no: u32) -> ItemRef {
    ItemRef {
        result_version: version.clone(),
        item_no,
    }
}

#[test]
fn multi_root_file_projection_uses_pinned_item_and_keeps_missing_evidence_unknown()
-> Result<(), String> {
    let store = IngestSink::default();
    let scope = id(1)?;
    let old_id = id(2)?;
    let new_id = id(3)?;
    let mut build = store
        .open_forest(scope.clone(), vec![root(b"/a", 1), root(b"/b", 2)])
        .map_err(|e| format!("{e:?}"))?;
    for index in 0..2 {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
        build
            .add_with_space_at(
                index,
                Observation {
                    path: vec![b"same".to_vec()],
                    size: Some(logical(10 + u64::from(index))),
                },
                SpaceObservation {
                    allocated: None,
                    identity: None,
                    link_count: None,
                    sharing: SharingEvidence::Unknown,
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .record_gap_at(1, vec![b"unread".to_vec()], "scan.path_denied".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    let old = build
        .publish_partial(old_id.clone())
        .map_err(|e| format!("{e:?}"))?;
    let first = reference(&old_id, 2);
    let second = reference(&old_id, 3);
    assert_eq!(
        old.observed_root_for(&first)
            .map_err(|e| format!("{e:?}"))?
            .1
            .volume_id,
        vec![1]
    );
    assert_eq!(
        old.observed_root_for(&second)
            .map_err(|e| format!("{e:?}"))?
            .1
            .volume_id,
        vec![2]
    );
    let query = Query(store.clone());
    let caller = CallerCtx::new(Channel::Gui, id(9)?, None);
    let request = BrowseDetailRequest {
        result_version: old_id.clone(),
        item: second.clone(),
    };
    query
        .check_scope(&request, &caller)
        .map_err(|e| format!("{e:?}"))?;
    let admitted = query
        .execute(request.clone(), &caller)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(admitted.item, second);
    assert_unknown_file_values(&old, &first, &second)?;
    let folder = project(
        &old,
        old.space_detail(&reference(&old_id, 1))
            .map_err(|e| format!("{e:?}"))?,
    );
    assert!(
        matches!(folder.logical, BrowseValue::Present { value } if matches!(value.size.amount, SizeAmount::LowerBound { .. }))
    );
    let new = build
        .seal(new_id.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(new.completeness(), ResultCompleteness::Missing);
    assert_eq!(
        query.execute(request, &caller).err(),
        Some(EngineError::reason("result.requery_required"))
    );
    let mismatch = BrowseDetailRequest {
        result_version: new_id.clone(),
        item: first.clone(),
    };
    assert_eq!(
        query.check_scope(&mismatch, &caller).err(),
        Some(EngineError::reason("result.requery_required"))
    );
    assert!(matches!(
        store.pin_current(&old_id),
        Err(du_analysis_index::QueryError::StaleVersion)
    ));
    assert_eq!(
        old.space_detail(&first)
            .map_err(|e| format!("{e:?}"))?
            .logical,
        Some(logical(10))
    );
    assert!(matches!(
        new.space_detail(&first),
        Err(du_analysis_index::QueryError::StaleVersion)
    ));
    assert_eq!(
        new.space_detail(&reference(&new_id, 3))
            .map_err(|e| format!("{e:?}"))?
            .logical,
        Some(logical(11))
    );
    Ok(())
}

fn assert_unknown_file_values(
    version: &ResultVersion,
    first: &ItemRef,
    second: &ItemRef,
) -> Result<(), String> {
    for (item, bytes) in [(&first, 10), (&second, 11)] {
        let projected = project(
            version,
            version.space_detail(item).map_err(|e| format!("{e:?}"))?,
        );
        assert!(
            matches!(projected.logical, BrowseValue::Present { value } if value.size == logical(bytes))
        );
        assert!(matches!(projected.allocated, BrowseValue::Unknown { .. }));
        assert!(matches!(
            projected.unique_physical,
            BrowseValue::Unknown { .. }
        ));
        assert!(matches!(projected.exclusive, BrowseValue::Unknown { .. }));
        assert!(matches!(projected.shared, BrowseValue::Unknown { .. }));
    }
    Ok(())
}
