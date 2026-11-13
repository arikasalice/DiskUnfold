//! Source evidence only: these checks do not authorize a share or define a scope contract.
use du_analysis_index::{
    FileIdentity, IngestSink, Observation, QueryError, SharingEvidence, SpaceObservation,
};
use du_core_contracts::result::ResultVersionRead;
use du_core_contracts::{
    ByteSize, Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use du_engine_api::current_analysis::{Error, admit_selected};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn publish(
    store: &IngestSink,
    lineage: &Id,
    scope: Id,
    version: Id,
    volume: u8,
    root_bytes: &[u8],
) -> Result<(), String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(root_bytes),
        },
        volume_id: vec![volume],
        file_id: vec![volume],
    };
    let mut build = store
        .open_forest_in_lineage(scope, vec![root], lineage.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![b"child".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(version, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

fn size_value(basis: SizeBasis, evidence: SizeEvidence, amount: SizeAmount) -> SizeValue {
    SizeValue {
        basis,
        evidence,
        amount,
    }
}

fn publish_size_evidence(store: &IngestSink) -> Result<Id, String> {
    let lineage = store
        .bind_current_analysis("basis-evidence", id(10)?)
        .map_err(|error| format!("bind lineage: {error:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/basis"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = store
        .open_forest_in_lineage(id(11)?, vec![root], lineage.clone())
        .map_err(|error| format!("open result: {error:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|error| format!("add root: {error:?}"))?;
    build
        .add_with_space_at(
            0,
            Observation {
                path: vec![b"unknown-logical".to_vec()],
                size: Some(size_value(
                    SizeBasis::Logical,
                    SizeEvidence::Unknown,
                    SizeAmount::Unknown,
                )),
            },
            SpaceObservation {
                allocated: Some(size_value(
                    SizeBasis::Allocated,
                    SizeEvidence::OsReported,
                    SizeAmount::LowerBound {
                        bytes: ByteSize::from(12_u64),
                    },
                )),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![2],
                }),
                link_count: Some(1),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(|error| format!("add basis-specific file: {error:?}"))?;
    build
        .add_file_at(
            0,
            Observation {
                path: vec![b"missing-size".to_vec()],
                size: None,
            },
        )
        .map_err(|error| format!("add missing-size file: {error:?}"))?;
    build
        .seal(id(12)?, ResultCompleteness::Complete)
        .map_err(|error| format!("seal result: {error:?}"))?;
    id(12)
}

#[test]
fn pinned_source_retains_its_own_item_path_and_volume_after_replacement() -> Result<(), String> {
    let store = IngestSink::default();
    let lineage = store
        .bind_current_analysis("fixture", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    publish(&store, &lineage, id(2)?, id(3)?, 1, b"/first")?;
    let old = admit_selected(&store, &id(3)?, None).map_err(|e| format!("{e:?}"))?;
    let old_item = ItemRef {
        result_version: id(3)?,
        item_no: 1,
    };
    let (_, old_root) = old
        .version()
        .observed_root_for(&old_item)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(old_root.volume_id, [1]);
    assert_eq!(
        old.version()
            .native_path(&old_item)
            .map_err(|e| format!("{e:?}"))?
            .raw
            .bytes()
            .map_err(|e| e.to_string())?,
        b"/first/child"
    );

    publish(&store, &lineage, id(4)?, id(5)?, 2, b"/second")?;
    let new = admit_selected(&store, &id(5)?, None).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        admit_selected(&store, &id(3)?, None).err(),
        Some(Error::StaleVersion)
    );
    assert_eq!(
        new.version().native_path(&old_item),
        Err(QueryError::StaleVersion)
    );
    assert_eq!(
        new.version().observed_root_for(&old_item).err(),
        Some(QueryError::StaleVersion)
    );
    assert_eq!(
        old.version()
            .native_path(&old_item)
            .map_err(|e| format!("{e:?}"))?
            .raw
            .bytes()
            .map_err(|e| e.to_string())?,
        b"/first/child"
    );
    let new_item = ItemRef {
        result_version: id(5)?,
        item_no: 1,
    };
    let (_, new_root) = new
        .version()
        .observed_root_for(&new_item)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(new_root.volume_id, [2]);
    assert_eq!(
        new.version()
            .native_path(&new_item)
            .map_err(|e| format!("{e:?}"))?
            .raw
            .bytes()
            .map_err(|e| e.to_string())?,
        b"/second/child"
    );
    Ok(())
}

#[test]
fn pinned_item_sizes_keep_basis_specific_unknown_lower_bound_and_none() -> Result<(), String> {
    let store = IngestSink::default();
    let revision = publish_size_evidence(&store)?;
    let source = admit_selected(&store, &revision, Some(&revision))
        .map_err(|error| format!("pin basis evidence: {error:?}"))?;
    let find_file = |name: &[u8]| {
        source
            .version()
            .items()
            .iter()
            .find(|item| item.name == name)
            .map(|item| item.item_no)
            .ok_or_else(|| format!("missing file item {name:?}"))
    };

    let unknown_item = find_file(b"unknown-logical")?;
    let logical = source
        .version()
        .item_size(unknown_item, SizeBasis::Logical)
        .ok_or("logical unknown evidence was dropped")?;
    assert_eq!(logical.basis, SizeBasis::Logical);
    assert_eq!(logical.evidence, SizeEvidence::Unknown);
    assert_eq!(logical.amount, SizeAmount::Unknown);
    let allocated = source
        .version()
        .item_size(unknown_item, SizeBasis::Allocated)
        .ok_or("allocated lower-bound evidence was dropped")?;
    assert_eq!(allocated.basis, SizeBasis::Allocated);
    assert_eq!(allocated.evidence, SizeEvidence::OsReported);
    assert_eq!(
        allocated.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(12_u64)
        }
    );
    assert!(
        source
            .version()
            .item_size(unknown_item, SizeBasis::UniquePhysical)
            .is_none()
    );

    let missing_item = find_file(b"missing-size")?;
    for basis in [
        SizeBasis::Logical,
        SizeBasis::Allocated,
        SizeBasis::UniquePhysical,
    ] {
        assert!(source.version().item_size(missing_item, basis).is_none());
    }
    Ok(())
}
