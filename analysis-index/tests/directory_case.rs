use du_analysis_index::{BuildError, IngestSink, Observation, ResultVersion};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, Timestamp,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    scan_observation::{DirectoryCaseEvidence, DirectoryCaseState},
};

fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}
fn root(name: &[u8]) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(name),
        },
        volume_id: vec![1],
        file_id: name.to_vec(),
    }
}
fn evidence(
    state: DirectoryCaseState,
    reason: Option<&str>,
) -> Result<DirectoryCaseEvidence, String> {
    Ok(DirectoryCaseEvidence {
        state,
        observed_at: Some(Timestamp::parse("2026-11-10T12:34:56.000Z")?),
        reason_key: reason.map(str::to_owned),
    })
}
fn reference(version: &ResultVersion, root_index: usize, name: &[u8]) -> Result<ItemRef, String> {
    let root = version.scope_roots()[root_index].item_no;
    let item = version
        .items()
        .iter()
        .find(|item| item.parent == Some(root) && item.name == name)
        .ok_or("missing item")?;
    Ok(ItemRef {
        result_version: version.version_id().clone(),
        item_no: item.item_no,
    })
}

#[test]
fn preserves_sparse_per_directory_evidence_across_pinned_publications() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1), vec![root(b"/one"), root(b"/two")])
        .map_err(|e| format!("{e:?}"))?;
    for root in 0..2 {
        build
            .add_directory_at(root, vec![])
            .map_err(|e| format!("{e:?}"))?;
        build
            .add_directory_at(root, vec![b"same".to_vec()])
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .add_directory_at(0, vec![b"unsupported".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![b"failed".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![b"uncollected".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_file_at(
            0,
            Observation {
                path: vec![b"file".to_vec()],
                size: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    let baseline = build.publish_partial(id(2)).map_err(|e| format!("{e:?}"))?;
    let sensitive = evidence(DirectoryCaseState::Sensitive, None)?;
    let insensitive = evidence(DirectoryCaseState::Insensitive, None)?;
    let unsupported = evidence(DirectoryCaseState::Unsupported, Some("os.unsupported"))?;
    let failed = evidence(DirectoryCaseState::Failed, Some("scan.access_denied"))?;
    build
        .set_directory_case_evidence(0, &[], sensitive.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_directory_case_evidence(0, &[b"same".to_vec()], sensitive.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_directory_case_evidence(1, &[b"same".to_vec()], insensitive.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_directory_case_evidence(0, &[b"unsupported".to_vec()], unsupported.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_directory_case_evidence(0, &[b"failed".to_vec()], failed.clone())
        .map_err(|e| format!("{e:?}"))?;
    let partial = build.publish_partial(id(3)).map_err(|e| format!("{e:?}"))?;
    assert_eq!(baseline.scope_total(), partial.scope_total());
    assert_eq!(baseline.items(), partial.items());
    assert_eq!(
        baseline.directory_case_evidence(&reference(&baseline, 0, b"same")?),
        None
    );
    assert_eq!(
        partial.directory_case_evidence(&ItemRef {
            result_version: id(3),
            item_no: partial.scope_roots()[0].item_no
        }),
        Some(&sensitive)
    );
    for (root, name, expected) in [
        (0, b"same".as_slice(), &sensitive),
        (1, b"same".as_slice(), &insensitive),
        (0, b"unsupported".as_slice(), &unsupported),
        (0, b"failed".as_slice(), &failed),
    ] {
        assert_eq!(
            partial.directory_case_evidence(&reference(&partial, root, name)?),
            Some(expected)
        );
    }
    for name in [b"file".as_slice(), b"uncollected".as_slice()] {
        assert_eq!(
            partial.directory_case_evidence(&reference(&partial, 0, name)?),
            None
        );
    }
    let same = reference(&partial, 0, b"same")?;
    assert_eq!(
        partial.directory_case_evidence(&ItemRef {
            result_version: id(9),
            ..same.clone()
        }),
        None
    );
    assert_eq!(
        partial.directory_case_evidence(&ItemRef {
            result_version: id(3),
            item_no: u32::MAX
        }),
        None
    );
    build
        .set_directory_case_evidence(0, &[b"same".to_vec()], insensitive.clone())
        .map_err(|e| format!("{e:?}"))?;
    let final_version = build
        .seal(id(4), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(partial.directory_case_evidence(&same), Some(&sensitive));
    assert_eq!(
        final_version.directory_case_evidence(&reference(&final_version, 0, b"same")?),
        Some(&insensitive)
    );
    assert_eq!(final_version.directory_case_evidence(&same), None);
    Ok(())
}

#[test]
fn rejects_unregistered_wrong_root_file_and_unknown_structure_without_transfer()
-> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(10), vec![root(b"/one"), root(b"/two")])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(1, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_file_at(
            0,
            Observation {
                path: vec![b"file".to_vec()],
                size: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_at(
            0,
            Observation {
                path: vec![b"unknown".to_vec()],
                size: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_structure(vec![b"structure".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![b"only".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let case = evidence(DirectoryCaseState::Sensitive, None)?;
    for (root_index, path) in [
        (0, vec![b"file".to_vec()]),
        (0, vec![b"unknown".to_vec()]),
        (0, vec![b"structure".to_vec()]),
        (1, vec![b"only".to_vec()]),
        (0, vec![b"missing".to_vec()]),
        (2, vec![]),
    ] {
        assert_eq!(
            build.set_directory_case_evidence(root_index, &path, case.clone()),
            Err(BuildError::InvalidTree)
        );
    }
    let result = build
        .seal(id(11), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        result.directory_case_evidence(&reference(&result, 0, b"only")?),
        None
    );
    let mut stopped = sink.open(id(12)).map_err(|e| format!("{e:?}"))?;
    stopped
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    stopped.abort();
    assert_eq!(
        stopped.set_directory_case_evidence(0, &[], case),
        Err(BuildError::Unavailable)
    );
    Ok(())
}

#[test]
fn duplicate_observation_is_rejected_without_mutating_the_pinned_partial() -> Result<(), String> {
    for replace_with_file in [false, true] {
        let sink = IngestSink::default();
        let mut build = sink.open(id(30)).map_err(|e| format!("{e:?}"))?;
        build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        let path = vec![b"dir".to_vec()];
        build
            .add_directory(path.clone())
            .map_err(|e| format!("{e:?}"))?;
        let case = evidence(DirectoryCaseState::Sensitive, None)?;
        build
            .set_directory_case_evidence(0, &path, case.clone())
            .map_err(|e| format!("{e:?}"))?;
        let partial = build
            .publish_partial(id(31))
            .map_err(|e| format!("{e:?}"))?;
        let reference = reference(&partial, 0, b"dir")?;
        let result = if replace_with_file {
            build.add_file(Observation { path, size: None })
        } else {
            build.add_directory(path)
        };
        assert_eq!(result, Err(BuildError::InvalidTree));
        assert_eq!(partial.directory_case_evidence(&reference), Some(&case));
        assert_eq!(
            build.publish_partial(id(32)).err(),
            Some(BuildError::Unavailable)
        );
    }
    Ok(())
}
