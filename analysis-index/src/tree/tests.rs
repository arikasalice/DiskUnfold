//! Crate-internal checks of how a build shares file space evidence.
use crate::{
    BuildHandle, FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence,
    SpaceObservation,
};
use du_core_contracts::result::{
    ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead,
};
use du_core_contracts::{
    ByteSize, Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
};
use std::sync::Arc;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).map_err(|e| format!("{e:?}"))
}

fn forest_root(letter: u8) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(&[b'/', letter]),
        },
        volume_id: vec![letter],
        file_id: vec![letter, 1],
    }
}

fn populated_forest() -> Result<BuildHandle, String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(10)?, vec![forest_root(b'a'), forest_root(b'b')])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(1, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_with_space_at(
            0,
            Observation {
                path: vec![b"z".to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    amount: SizeAmount::Exact {
                        bytes: ByteSize::from(12),
                    },
                    evidence: SizeEvidence::Measured,
                }),
            },
            SpaceObservation {
                allocated: None,
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![9],
                }),
                link_count: Some(1),
                sharing: SharingEvidence::Unknown,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .record_gap_at(1, vec![], "scan.path_denied".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    Ok(build)
}

#[test]
fn partial_versions_share_file_evidence_and_seal_compactly() -> Result<(), String> {
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
    build.add_with_space(file, space.clone()).map_err(failed)?;
    let first = build.publish_partial(id(2)?).map_err(failed)?;
    let second = build.publish_partial(id(3)?).map_err(failed)?;
    let a = first
        .physical
        .as_ref()
        .and_then(|values| values.get(1))
        .ok_or("first version lost the file evidence")?;
    let b = second
        .physical
        .as_ref()
        .and_then(|values| values.get(1))
        .ok_or("second version lost the file evidence")?;
    assert_eq!(a.to_owned(), space);
    assert_eq!(b.to_owned(), space);
    assert!(matches!(
        (a, b),
        (
            crate::physical::PhysicalObservationRef::Shared(a),
            crate::physical::PhysicalObservationRef::Shared(b)
        ) if std::ptr::eq(a, b)
    ));
    let sealed = build
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(failed)?;
    assert!(
        sealed
            .physical
            .as_ref()
            .is_some_and(crate::physical::PhysicalObservations::is_compact)
    );
    Ok(())
}

#[test]
fn inserted_node_invalidates_order_and_preserves_old_snapshot() -> Result<(), String> {
    let mut build = populated_forest()?;
    let first = build
        .publish_partial(id(11)?)
        .map_err(|e| format!("{e:?}"))?;
    let first_order = build
        .numbered_nodes
        .as_ref()
        .ok_or("partial publication did not cache its node order")?
        .clone();
    assert_eq!(first.items().len(), 3);
    assert_eq!(names(&first), [b"".as_slice(), b"", b"z"]);
    assert_eq!(first.root_indices, [0, 1, 0]);
    assert_eq!(first.scope_gaps().len(), 1);
    assert_eq!(physical_identity(&first, 2), Some(&[9][..]));
    assert_eq!(first_order.as_slice(), &[0, 1, 2]);

    build
        .add_file_at(
            0,
            Observation {
                path: vec![b"a".to_vec()],
                size: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    assert!(build.numbered_nodes.is_none());

    let second = build
        .publish_partial(id(12)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(names(&second), [b"".as_slice(), b"".as_slice(), b"a", b"z"]);
    assert_eq!(second.root_indices, [0, 1, 0, 0]);
    assert_eq!(second.scope_gaps().len(), 1);
    assert_eq!(physical_identity(&second, 2), None);
    assert_eq!(physical_identity(&second, 3), Some(&[9][..]));
    assert_eq!(names(&first), [b"".as_slice(), b"", b"z"]);
    assert_eq!(first.root_indices, [0, 1, 0]);
    assert_eq!(first.scope_gaps().len(), 1);
    assert_eq!(physical_identity(&first, 2), Some(&[9][..]));

    let second_order = build
        .numbered_nodes
        .as_ref()
        .ok_or("second partial publication did not cache its node order")?
        .clone();
    assert_eq!(second_order.as_slice(), &[0, 1, 3, 2]);
    assert!(!Arc::ptr_eq(&first_order, &second_order));
    Ok(())
}

#[test]
fn cached_node_order_is_reused_through_seal() -> Result<(), String> {
    let mut build = populated_forest()?;
    let first = build
        .publish_partial(id(11)?)
        .map_err(|e| format!("{e:?}"))?;
    let cached_order = build
        .numbered_nodes
        .as_ref()
        .ok_or("partial publication did not cache its node order")?
        .clone();
    let second = build
        .publish_partial(id(12)?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(Arc::ptr_eq(
        &cached_order,
        build
            .numbered_nodes
            .as_ref()
            .ok_or("missing cached order")?
    ));
    let sealed = build
        .publish(id(13)?, ResultCompleteness::Complete, None, true)
        .map_err(|e| format!("{e:?}"))?;
    assert!(Arc::ptr_eq(
        &cached_order,
        build.numbered_nodes.as_ref().ok_or("missing order")?
    ));
    assert_eq!(names(&first), names(&second));
    assert_eq!(names(&sealed), names(&second));
    assert_eq!(sealed.root_indices, second.root_indices);
    assert_eq!(sealed.scope_gaps(), second.scope_gaps());
    assert_eq!(physical_identity(&sealed, 2), Some(&[9][..]));
    Ok(())
}

#[test]
fn abort_releases_cached_order_and_keeps_the_published_snapshot_immutable() -> Result<(), String> {
    use crate::BuildError;

    let sink = IngestSink::default();
    let mut build = sink.open(id(20)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;

    let partial = build
        .publish_partial(id(21)?)
        .map_err(|e| format!("{e:?}"))?;
    let weak_order = Arc::downgrade(
        build
            .numbered_nodes
            .as_ref()
            .ok_or("partial publication did not cache its node order")?,
    );

    build.abort();

    assert!(weak_order.upgrade().is_none());
    assert_eq!(
        build.publish_partial(id(22)?).err(),
        Some(BuildError::Unavailable)
    );
    assert_eq!(names(&partial), [b"".as_slice(), b"file"]);
    assert_eq!(partial.root_indices, [0, 0]);
    Ok(())
}

fn names(version: &ResultVersion) -> Vec<&[u8]> {
    version
        .items()
        .iter()
        .map(|item| item.name.as_slice())
        .collect()
}

fn physical_identity(version: &ResultVersion, item: usize) -> Option<&[u8]> {
    version
        .physical
        .as_ref()?
        .get(u32::try_from(item).ok()?)?
        .identity()
        .map(|identity| identity.file)
}
