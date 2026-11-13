use super::super::{id, reference, root, value};
use du_analysis_index::{IngestSink, Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    SizeAmount, SizeBasis, SizeEvidence, SizeValue, result::ResultCompleteness,
};

fn add_observation(
    build: &mut du_analysis_index::BuildHandle,
    name: &str,
    size: Option<SizeValue>,
    allocated: Option<SizeValue>,
) -> Result<(), String> {
    build
        .add_with_space_at(
            0,
            Observation {
                path: vec![name.as_bytes().to_vec()],
                size,
            },
            SpaceObservation {
                allocated,
                identity: None,
                link_count: None,
                sharing: SharingEvidence::Unknown,
            },
        )
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn item_size_rejects_mismatched_bases_without_inventing_values() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root(1, "a")])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;

    add_observation(
        &mut build,
        "mixed",
        Some(value(SizeBasis::Allocated, 13)),
        Some(value(SizeBasis::Logical, 7)),
    )?;
    add_observation(
        &mut build,
        "valid",
        Some(value(SizeBasis::Logical, 11)),
        Some(value(SizeBasis::Allocated, 8)),
    )?;
    let mut unknown = value(SizeBasis::Logical, 0);
    unknown.evidence = SizeEvidence::Unknown;
    unknown.amount = SizeAmount::Unknown;
    add_observation(&mut build, "unknown", Some(unknown.clone()), None)?;
    add_observation(&mut build, "missing", None, None)?;

    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mixed = reference(&result, "mixed")?;
    assert!(
        result
            .item_size(mixed.item_no, SizeBasis::Logical)
            .is_none()
    );
    assert!(
        result
            .item_size(mixed.item_no, SizeBasis::Allocated)
            .is_none()
    );
    let mixed_detail = result.space_detail(&mixed).map_err(|e| format!("{e:?}"))?;
    assert!(mixed_detail.logical.is_none());
    assert!(mixed_detail.allocated.is_none());

    let valid = reference(&result, "valid")?;
    assert_eq!(
        result.item_size(valid.item_no, SizeBasis::Logical),
        Some(&value(SizeBasis::Logical, 11))
    );
    assert_eq!(
        result.item_size(valid.item_no, SizeBasis::Allocated),
        Some(&value(SizeBasis::Allocated, 8))
    );
    let valid_detail = result.space_detail(&valid).map_err(|e| format!("{e:?}"))?;
    assert_eq!(valid_detail.logical, Some(value(SizeBasis::Logical, 11)));
    assert_eq!(valid_detail.allocated, Some(value(SizeBasis::Allocated, 8)));

    let unknown_ref = reference(&result, "unknown")?;
    assert_eq!(
        result.item_size(unknown_ref.item_no, SizeBasis::Logical),
        Some(&unknown)
    );
    assert_eq!(
        result
            .space_detail(&unknown_ref)
            .map_err(|e| format!("{e:?}"))?
            .logical,
        Some(unknown)
    );

    let missing = reference(&result, "missing")?;
    assert!(
        result
            .item_size(missing.item_no, SizeBasis::Logical)
            .is_none()
    );
    assert!(
        result
            .item_size(missing.item_no, SizeBasis::Allocated)
            .is_none()
    );
    let missing_detail = result
        .space_detail(&missing)
        .map_err(|e| format!("{e:?}"))?;
    assert!(missing_detail.logical.is_none());
    assert!(missing_detail.allocated.is_none());
    Ok(())
}
