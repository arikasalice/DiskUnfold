use du_analysis_index::{
    IngestSink,
    visualization::{
        group_page::{Error, PinnedGroup},
        live_models::{GroupRows, SizeRange},
    },
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
    })
}

#[test]
fn admitted_group_preserves_rows_evidence_and_version_after_replacement() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    let version = builder
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let rows = vec![
        row(version.version_id(), "first")?,
        row(version.version_id(), "second")?,
    ];
    let mut total = rows[0].size.clone();
    total.amount = SizeAmount::Exact {
        bytes: 10_u64.into(),
    };
    let group = GroupRows {
        rows: rows.clone(),
        total: total.clone(),
        size_range: SizeRange::Unknown,
    };
    version
        .insert_viz_rows("model".into(), vec![], HashMap::from([("g".into(), group)]))
        .map_err(|e| format!("{e:?}"))?;
    let current = sink
        .pin_current(version.version_id())
        .map_err(|e| format!("{e:?}"))?;
    let pinned = PinnedGroup::from_admitted(current, "model", "g").map_err(|e| format!("{e:?}"))?;
    version
        .remove_viz_rows("model")
        .map_err(|e| format!("{e:?}"))?;
    let mut replacement = sink.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    let _new = replacement
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let page = pinned
        .page(1, 1, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.rows, vec![rows[1].clone()]);
    assert_eq!(page.total_count, 2);
    assert_eq!(page.group_total, total);
    assert_eq!(page.size_range, SizeRange::Unknown);
    assert_eq!(page.result_revision, id(2)?);
    assert!(sink.pin_current(&id(2)?).is_err());
    assert_eq!(
        PinnedGroup::from_admitted(version, "model", "g").err(),
        Some(Error::ModelExpired)
    );
    Ok(())
}

#[test]
fn group_missing_range_and_cancellation_are_distinct() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(5)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    let v = builder
        .seal(id(6)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let item = row(v.version_id(), "member")?;
    v.insert_viz_rows(
        "m".into(),
        vec![],
        HashMap::from([(
            "g".into(),
            GroupRows {
                rows: vec![item.clone(), item.clone()],
                total: item.size.clone(),
                size_range: SizeRange::Unknown,
            },
        )]),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        PinnedGroup::from_admitted(v.clone(), "m", "other").err(),
        Some(Error::GroupMissing)
    );
    let pin = PinnedGroup::from_admitted(v, "m", "g").map_err(|e| format!("{e:?}"))?;
    assert!(
        pin.page(2, 1, &mut || false)
            .map_err(|e| format!("{e:?}"))?
            .rows
            .is_empty()
    );
    for (start, count) in [(3, 1), (0, 0), (0, 1001)] {
        assert_eq!(
            pin.page(start, count, &mut || false).err(),
            Some(Error::InvalidRange)
        );
    }
    let mut checks = 0;
    assert_eq!(
        pin.page(0, 2, &mut || {
            checks += 1;
            checks == 3
        })
        .err(),
        Some(Error::Cancelled)
    );
    Ok(())
}
