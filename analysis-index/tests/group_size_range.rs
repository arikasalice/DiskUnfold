use du_analysis_index::{
    IngestSink,
    visualization::{
        group_page::PinnedGroup,
        live_models::{GroupRows, PrepareError, PreparedRows, SizeRange},
    },
};
use du_core_contracts::{
    Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::ResultCompleteness,
    visualization::{
        quantity::VizQuantity,
        rows::{VizDenominator, VizDenominatorBasis, VizRow},
    },
};
use std::{collections::HashMap, sync::Arc};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn size(basis: SizeBasis, evidence: SizeEvidence, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}

fn row(value: SizeValue) -> Result<VizRow, String> {
    Ok(VizRow {
        item_ref: None,
        group_ref: Some("g".into()),
        display_name: "member".into(),
        size: value,
        quantity: None,
        ratio: None,
        denominator: VizDenominator {
            basis: VizDenominatorBasis::Scope,
            value: VizQuantity::exact("bytes", 5).ok_or("invalid quantity")?,
            size: None,
        },
        legend_class_key: "x".into(),
        display_flags: vec![],
    })
}

fn setup(
    values: Vec<SizeValue>,
    range: SizeRange,
    seed: u32,
    completeness: ResultCompleteness,
) -> Result<SizeRange, String> {
    setup_with_total(
        values,
        range,
        size(SizeBasis::Logical, SizeEvidence::Measured, 10),
        seed,
        completeness,
    )
}

fn setup_with_total(
    values: Vec<SizeValue>,
    range: SizeRange,
    total: SizeValue,
    seed: u32,
    completeness: ResultCompleteness,
) -> Result<SizeRange, String> {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(seed)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    let version = builder
        .seal(id(seed + 1)?, completeness)
        .map_err(|e| format!("{e:?}"))?;
    let rows = values.into_iter().map(row).collect::<Result<Vec<_>, _>>()?;
    let prepared = PreparedRows::new(
        vec![],
        HashMap::from([(
            "g".into(),
            GroupRows {
                rows,
                total,
                size_range: range,
            },
        )]),
    );
    let mut prepared = Some(prepared);
    version
        .insert_prepared_viz_rows("m".into(), &mut prepared)
        .map_err(|e| format!("{e:?}"))?;
    let pin =
        PinnedGroup::from_admitted(Arc::clone(&version), "m", "g").map_err(|e| format!("{e:?}"))?;
    let page = pin
        .page(0, 10, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    Ok(page.size_range)
}

fn known() -> SizeRange {
    SizeRange::Known {
        min: size(SizeBasis::Logical, SizeEvidence::Measured, 5),
        max: size(SizeBasis::Logical, SizeEvidence::Measured, 10),
    }
}

#[test]
fn known_group_range_is_withheld_for_unobserved_or_mixed_basis_members() -> Result<(), String> {
    let unobserved = SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Unknown,
        amount: SizeAmount::Unknown,
    };
    assert_eq!(
        setup(vec![unobserved], known(), 1, ResultCompleteness::Complete)?,
        SizeRange::Unknown
    );

    let mixed_basis = size(SizeBasis::Allocated, SizeEvidence::Measured, 5);
    assert_eq!(
        setup(vec![mixed_basis], known(), 3, ResultCompleteness::Complete)?,
        SizeRange::Unknown
    );

    let fully_observed = size(SizeBasis::Logical, SizeEvidence::Measured, 5);
    assert_eq!(
        setup(
            vec![fully_observed],
            known(),
            7,
            ResultCompleteness::Partial
        )?,
        SizeRange::Unknown
    );
    Ok(())
}

#[test]
fn homogeneous_observed_members_preserve_producer_range_verbatim() -> Result<(), String> {
    let members = vec![
        size(SizeBasis::Logical, SizeEvidence::Measured, 5),
        size(SizeBasis::Logical, SizeEvidence::Estimated, 8),
    ];
    let range = SizeRange::Known {
        min: size(SizeBasis::Logical, SizeEvidence::Measured, 5),
        max: size(SizeBasis::Logical, SizeEvidence::Measured, 8),
    };
    assert_eq!(
        setup(members, range.clone(), 5, ResultCompleteness::Complete)?,
        range
    );
    Ok(())
}

#[test]
fn known_range_requires_exact_valid_bounds_and_group_total_basis() -> Result<(), String> {
    let members = vec![
        size(SizeBasis::Logical, SizeEvidence::Measured, 5),
        size(SizeBasis::Logical, SizeEvidence::Estimated, 8),
    ];
    let reversed = SizeRange::Known {
        min: size(SizeBasis::Logical, SizeEvidence::Measured, 9),
        max: size(SizeBasis::Logical, SizeEvidence::Measured, 8),
    };
    assert_eq!(
        setup(members.clone(), reversed, 11, ResultCompleteness::Complete)?,
        SizeRange::Unknown
    );

    let inexact_min = SizeRange::Known {
        min: SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount: SizeAmount::LowerBound { bytes: 5.into() },
        },
        max: size(SizeBasis::Logical, SizeEvidence::Measured, 8),
    };
    assert_eq!(
        setup(
            members.clone(),
            inexact_min,
            13,
            ResultCompleteness::Complete
        )?,
        SizeRange::Unknown
    );

    let unknown_bound = SizeRange::Known {
        min: SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Unknown,
            amount: SizeAmount::Exact { bytes: 5.into() },
        },
        max: size(SizeBasis::Logical, SizeEvidence::Measured, 8),
    };
    assert_eq!(
        setup(
            members.clone(),
            unknown_bound,
            15,
            ResultCompleteness::Complete
        )?,
        SizeRange::Unknown
    );

    assert_eq!(
        setup_with_total(
            members,
            known(),
            size(SizeBasis::Allocated, SizeEvidence::Measured, 10),
            17,
            ResultCompleteness::Complete,
        )?,
        SizeRange::Unknown
    );
    Ok(())
}

#[test]
fn known_range_must_match_all_provided_member_extrema_and_empty_is_unknown() -> Result<(), String> {
    let members = vec![
        size(SizeBasis::Logical, SizeEvidence::Measured, 5),
        size(SizeBasis::Logical, SizeEvidence::Measured, 8),
    ];
    let too_narrow = SizeRange::Known {
        min: size(SizeBasis::Logical, SizeEvidence::Measured, 6),
        max: size(SizeBasis::Logical, SizeEvidence::Measured, 8),
    };
    assert_eq!(
        setup(
            members.clone(),
            too_narrow,
            19,
            ResultCompleteness::Complete
        )?,
        SizeRange::Unknown
    );
    let too_wide = SizeRange::Known {
        min: size(SizeBasis::Logical, SizeEvidence::Measured, 5),
        max: size(SizeBasis::Logical, SizeEvidence::Measured, 9),
    };
    assert_eq!(
        setup(members, too_wide, 21, ResultCompleteness::Complete)?,
        SizeRange::Unknown
    );
    assert_eq!(
        setup(vec![], known(), 23, ResultCompleteness::Complete)?,
        SizeRange::Unknown
    );
    Ok(())
}

#[test]
fn observed_zero_max_and_matching_partial_member_set_are_preserved() -> Result<(), String> {
    let range = SizeRange::Known {
        min: size(SizeBasis::Logical, SizeEvidence::Measured, 0),
        max: size(SizeBasis::Logical, SizeEvidence::OsReported, u64::MAX),
    };
    assert_eq!(
        setup(
            vec![
                size(SizeBasis::Logical, SizeEvidence::Measured, 0),
                size(SizeBasis::Logical, SizeEvidence::Estimated, u64::MAX),
            ],
            range.clone(),
            25,
            ResultCompleteness::Complete,
        )?,
        range
    );
    Ok(())
}

#[test]
fn cancellable_preparation_polls_during_member_traversal() -> Result<(), String> {
    let group = GroupRows {
        rows: vec![
            row(size(SizeBasis::Logical, SizeEvidence::Measured, 5))?,
            row(size(SizeBasis::Logical, SizeEvidence::Measured, 8))?,
        ],
        total: size(SizeBasis::Logical, SizeEvidence::Measured, 10),
        size_range: known(),
    };
    let mut calls = 0;
    let result =
        PreparedRows::new_cancellable(vec![], HashMap::from([("g".into(), group)]), &mut || {
            calls += 1;
            calls == 2
        });
    assert_eq!(result.err(), Some(PrepareError::Cancelled));
    assert_eq!(calls, 2);
    Ok(())
}
