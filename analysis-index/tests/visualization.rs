use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    visualization::{AreaValue, Expansion, HierarchyError, hierarchy},
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};
use std::sync::Arc;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|p| p.as_bytes().to_vec()).collect()
}
fn size(bytes: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}
fn build(
    completeness: ResultCompleteness,
) -> Result<Arc<ResultVersion>, Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    for dir in [&[][..], &["a"], &["a", "deep"]] {
        build
            .add_directory(path(dir))
            .map_err(|e| format!("{e:?}"))?;
    }
    for (parts, value) in [
        (&["a", "deep", "leaf"][..], Some(size(40))),
        (&["b"][..], Some(size(20))),
        (&["zero"][..], Some(size(0))),
        (&["unknown"][..], None),
    ] {
        build
            .add_file(Observation {
                path: path(parts),
                size: value,
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(2)?, completeness)
        .map_err(|e| format!("{e:?}").into())
}
fn reference(version: &ResultVersion, name: &[u8]) -> Result<ItemRef, Box<dyn std::error::Error>> {
    let item = version
        .items()
        .iter()
        .find(|item| item.name == name)
        .ok_or("missing item")?;
    Ok(ItemRef {
        result_version: version.version_id().clone(),
        item_no: item.item_no,
    })
}
fn expansion(item: ItemRef, extra_depth: i64) -> Expansion {
    Expansion { item, extra_depth }
}

#[test]
fn preserves_unknown_zero_lower_bound_and_depth_boundary() -> Result<(), Box<dyn std::error::Error>>
{
    let version = build(ResultCompleteness::Complete)?;
    let root = reference(&version, b"")?;
    let result = hierarchy(&version, &root, &root, SizeBasis::Logical, 1, &[])
        .map_err(|e| format!("{e:?}"))?;
    let names: Vec<_> = result
        .rows
        .iter()
        .map(|row| row.raw_name.as_slice())
        .collect();
    assert_eq!(names, vec![b"".as_slice(), b"a", b"b", b"unknown", b"zero"]);
    assert!(matches!(
        result.rows[0].value.amount,
        SizeAmount::LowerBound { .. }
    ));
    assert_eq!(result.rows[1].omitted_children, 1);
    assert_eq!(result.rows[3].area_value, AreaValue::Unknown);
    assert_eq!(result.rows[4].area_value, AreaValue::ExactZero);
    assert_eq!(result.rows[1].parent_item_no, Some(root.item_no));
    assert_eq!(result.rows[1].visible_depth, 1);
    let partial = build(ResultCompleteness::Partial)?;
    let root = reference(&partial, b"")?;
    let partial_rows = hierarchy(&partial, &root, &root, SizeBasis::Logical, 1, &[])
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        partial_rows
            .rows
            .iter()
            .find(|row| row.raw_name == b"zero")
            .ok_or("zero")?
            .area_value,
        AreaValue::LowerBoundZero
    );
    Ok(())
}

#[test]
fn expansions_validate_before_inactive_entries_are_removed()
-> Result<(), Box<dyn std::error::Error>> {
    let version = build(ResultCompleteness::Complete)?;
    let root = reference(&version, b"")?;
    let a = reference(&version, b"a")?;
    let deep = reference(&version, b"deep")?;
    let leaf = reference(&version, b"leaf")?;
    let plain = hierarchy(&version, &root, &root, SizeBasis::Logical, 1, &[])
        .map_err(|e| format!("{e:?}"))?;
    let explicit_zero = hierarchy(
        &version,
        &root,
        &root,
        SizeBasis::Logical,
        1,
        &[expansion(a.clone(), 0)],
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(plain, explicit_zero);
    let outside_navigation = hierarchy(
        &version,
        &root,
        &a,
        SizeBasis::Logical,
        1,
        &[expansion(root.clone(), 1)],
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(outside_navigation.active_expansions.is_empty());
    let hidden = hierarchy(
        &version,
        &root,
        &root,
        SizeBasis::Logical,
        1,
        &[expansion(deep.clone(), 8)],
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(hidden.active_expansions.is_empty());
    let shown = hierarchy(
        &version,
        &root,
        &root,
        SizeBasis::Logical,
        1,
        &[expansion(a.clone(), 1), expansion(deep.clone(), 1)],
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(
        shown
            .rows
            .iter()
            .any(|row| row.item == deep && row.visible_depth == 2)
    );
    assert!(!shown.rows.iter().any(|row| row.item == leaf));
    assert_eq!(shown.active_expansions.len(), 2);
    for invalid in [
        vec![expansion(a.clone(), 9)],
        vec![expansion(a.clone(), -1)],
        vec![expansion(a.clone(), 0), expansion(a.clone(), 1)],
        vec![expansion(leaf.clone(), 1)],
        vec![expansion(
            ItemRef {
                result_version: id(9)?,
                item_no: a.item_no,
            },
            1,
        )],
    ] {
        assert_eq!(
            hierarchy(&version, &root, &root, SizeBasis::Logical, 1, &invalid).err(),
            Some(HierarchyError::ViewportInvalid)
        );
    }
    assert_eq!(
        hierarchy(&version, &a, &root, SizeBasis::Logical, 1, &[]).err(),
        Some(HierarchyError::InvalidRoot)
    );
    assert_eq!(
        hierarchy(
            &version,
            &a,
            &a,
            SizeBasis::Logical,
            1,
            &[expansion(root.clone(), 1)]
        )
        .err(),
        Some(HierarchyError::ViewportInvalid)
    );
    Ok(())
}

#[test]
fn unknown_basis_never_becomes_zero_or_a_visible_area() -> Result<(), Box<dyn std::error::Error>> {
    let version = build(ResultCompleteness::Complete)?;
    let root = reference(&version, b"")?;
    let result = hierarchy(&version, &root, &root, SizeBasis::Allocated, 2, &[])
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.rows[0].area_value, AreaValue::Unknown);
    assert!(
        result
            .rows
            .iter()
            .skip(1)
            .all(|row| row.suppressed_by_parent)
    );
    assert!(
        result
            .rows
            .iter()
            .all(|row| row.value.basis == SizeBasis::Allocated)
    );
    Ok(())
}

#[test]
fn expansion_count_64_is_allowed_and_65_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    for n in 0..65 {
        builder
            .add_directory(vec![format!("d{n:02}").into_bytes()])
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = builder
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let root = reference(&version, b"")?;
    let expansions: Vec<_> = version
        .items()
        .iter()
        .skip(1)
        .map(|item| {
            expansion(
                ItemRef {
                    result_version: version.version_id().clone(),
                    item_no: item.item_no,
                },
                8,
            )
        })
        .collect();
    assert!(
        hierarchy(
            &version,
            &root,
            &root,
            SizeBasis::Logical,
            1,
            &expansions[..64]
        )
        .is_ok()
    );
    assert_eq!(
        hierarchy(&version, &root, &root, SizeBasis::Logical, 1, &expansions).err(),
        Some(HierarchyError::ViewportInvalid)
    );
    Ok(())
}

#[test]
fn eight_extra_levels_end_at_the_visible_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(5)?).map_err(|e| format!("{e:?}"))?;
    let mut current = Vec::new();
    builder
        .add_directory(current.clone())
        .map_err(|e| format!("{e:?}"))?;
    for n in 1..=9 {
        current.push(format!("d{n}").into_bytes());
        builder
            .add_directory(current.clone())
            .map_err(|e| format!("{e:?}"))?;
    }
    current.push(b"leaf".to_vec());
    builder
        .add_file(Observation {
            path: current,
            size: Some(size(1)),
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = builder
        .seal(id(6)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let root = reference(&version, b"")?;
    let rows = hierarchy(
        &version,
        &root,
        &root,
        SizeBasis::Logical,
        1,
        &[expansion(root.clone(), 8)],
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(rows.rows.len(), 10);
    assert_eq!(rows.rows.last().ok_or("last")?.visible_depth, 9);
    assert_eq!(rows.rows.last().ok_or("last")?.omitted_children, 1);
    assert!(!rows.rows.iter().any(|row| row.raw_name == b"leaf"));
    Ok(())
}

#[test]
fn hierarchy_stays_on_its_pinned_version_after_publication()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut first = sink.open(id(10)?).map_err(|e| format!("{e:?}"))?;
    first
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    first
        .add_file(Observation {
            path: path(&["first"]),
            size: Some(size(5)),
        })
        .map_err(|e| format!("{e:?}"))?;
    let pinned = first
        .seal(id(11)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let root = reference(&pinned, b"")?;
    let before = hierarchy(&pinned, &root, &root, SizeBasis::Logical, 1, &[])
        .map_err(|e| format!("{e:?}"))?;
    let mut second = sink.open(id(10)?).map_err(|e| format!("{e:?}"))?;
    second
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    second
        .add_file(Observation {
            path: path(&["second"]),
            size: Some(size(9)),
        })
        .map_err(|e| format!("{e:?}"))?;
    let current = second
        .seal(id(12)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let after = hierarchy(&pinned, &root, &root, SizeBasis::Logical, 1, &[])
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(before, after);
    assert_eq!(
        hierarchy(&current, &root, &root, SizeBasis::Logical, 1, &[]).err(),
        Some(HierarchyError::VersionMismatch)
    );
    Ok(())
}
