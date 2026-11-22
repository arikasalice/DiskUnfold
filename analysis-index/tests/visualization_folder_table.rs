//! The tables of expanded folders, kept with the rows of a model and read by the shape
//! reference of the folder.
use du_analysis_index::{
    IngestSink,
    visualization::live_models::{Error, PreparedRows},
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
    visualization::{
        quantity::VizQuantity,
        rows::{VizDenominator, VizDenominatorBasis, VizRow},
    },
};
use std::collections::HashMap;

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn row(version: &Id, name: &str) -> Result<VizRow, String> {
    Ok(VizRow {
        item_ref: Some(ItemRef {
            result_version: version.clone(),
            item_no: 0,
        }),
        group_ref: None,
        display_name: name.into(),
        size: SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount: SizeAmount::Exact {
                bytes: 5_u64.into(),
            },
        },
        quantity: None,
        ratio: None,
        denominator: VizDenominator {
            basis: VizDenominatorBasis::Scope,
            value: VizQuantity::exact("bytes", 5).ok_or("invalid quantity")?,
            size: None,
        },
        legend_class_key: "x".into(),
        display_flags: vec![],
        label_key: None,
        member_count: None,
        item_kind: None,
        condition: None,
        unread: None,
        type_slices: None,
    })
}

#[test]
fn a_folder_table_is_stored_with_the_model_and_pinned_by_its_shape_reference() -> Result<(), String>
{
    let sink = IngestSink::default();
    let mut builder = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    let version = builder
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let table = vec![
        row(version.version_id(), "first")?,
        row(version.version_id(), "second")?,
    ];
    let mut prepared = PreparedRows::new(vec![], HashMap::new());
    prepared
        .add_table("s1-1".into(), table.clone())
        .map_err(|e| format!("{e:?}"))?;
    // One table for each folder, and a folder needs a name.
    assert_eq!(
        prepared.add_table("s1-1".into(), table.clone()),
        Err(Error::Duplicate)
    );
    assert_eq!(
        prepared.add_table(String::new(), table.clone()),
        Err(Error::Duplicate)
    );
    version
        .insert_prepared_viz_rows("model".into(), &mut Some(prepared))
        .map_err(|e| format!("{e:?}"))?;

    let pinned = version
        .pin_viz_table("model", "s1-1")
        .map_err(|e| format!("{e:?}"))?
        .ok_or("no table")?;
    assert_eq!(pinned.to_vec(), table);
    // A folder that has no table is no error here: the caller decides what that means.
    assert!(
        version
            .pin_viz_table("model", "s2-1")
            .map_err(|e| format!("{e:?}"))?
            .is_none()
    );
    assert_eq!(
        version.pin_viz_table("other model", "s1-1").err(),
        Some(Error::Missing)
    );
    // The root rows are untouched by the table.
    assert!(
        version
            .pin_viz_rows("model")
            .map_err(|e| format!("{e:?}"))?
            .is_empty()
    );

    // A table read before the model is released stays readable, and is gone with it.
    version
        .remove_viz_rows("model")
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(pinned.len(), 2);
    assert_eq!(
        version.pin_viz_table("model", "s1-1").err(),
        Some(Error::Missing)
    );
    Ok(())
}
