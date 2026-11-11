use du_analysis_index::{
    IngestSink,
    visualization::{
        group_page::PinnedGroup,
        live_models::{GroupRows, PreparedRows, SizeRange},
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
    let sink = IngestSink::default();
    let mut builder = sink.open(id(seed)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    let version = builder
        .seal(id(seed + 1)?, completeness)
        .map_err(|e| format!("{e:?}"))?;
    let rows = values.into_iter().map(row).collect::<Result<Vec<_>, _>>()?;
    let total = size(SizeBasis::Logical, SizeEvidence::Measured, 10);
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
    let range = known();
    let members = vec![
        size(SizeBasis::Logical, SizeEvidence::Measured, 5),
        size(SizeBasis::Logical, SizeEvidence::Estimated, 8),
    ];
    assert_eq!(
        setup(members, range.clone(), 5, ResultCompleteness::Complete)?,
        range
    );
    Ok(())
}
