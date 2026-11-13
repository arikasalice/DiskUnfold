use du_analysis_index::{
    IngestSink, Observation,
    visualization::{
        bundle,
        geometry::{Rect, Scale},
        histogram::Basis,
        histogram_dispatch, registry,
        typed_layout::{Dispatcher, Strategy},
    },
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    size_units::{SI_UNITS, UNIT_TABLE_REVISION},
};
use du_engine_api::viz_pages::{
    histogram::{self, BinPresentation, PinnedRendered},
    production::RowPresentation,
};
fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}
#[test]
fn large_group_moves_members_and_cancels_during_preparation() -> Result<(), String> {
    let sink = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/large"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_forest(id(1), vec![root])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    for i in 0..4096 {
        build
            .add_file_at(
                0,
                Observation {
                    path: vec![format!("{i:05}").into_bytes()],
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact {
                            bytes: 5_u64.into(),
                        },
                    }),
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let scope = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let mut b = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_histogram_candidate(&mut b).map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(b.build(&[]).map_err(|e| format!("{e:?}"))?);
    let bounds = Rect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
    };
    let compute = || {
        PinnedRendered::compute(
            version.clone(),
            histogram_dispatch::Request {
                version: &version,
                scope: &scope,
                size_basis: SizeBasis::Logical,
                unit_table: &SI_UNITS,
                unit_table_revision: UNIT_TABLE_REVISION,
                bounds,
                basis: Basis::ItemCount,
                scale: Scale::Automatic,
                unknown_reason: "fixture.unknown",
            },
            &dispatcher,
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))
    };
    let selected = compute()?;
    let member_ptr = selected.rendered().prepared().model().histogram.bins[0]
        .members
        .as_ptr();
    let bins = selected.rendered().prepared().unit_bins().bins.clone();
    let request = || histogram::Request {
        version: version.clone(),
        scope: &scope,
        size_basis: SizeBasis::Logical,
        bins: &bins,
        bounds,
        scale: Scale::Automatic,
        fingerprint: "large".into(),
        unknown_reason: "fixture.unknown".into(),
        bin_presentation: (0..bins.len())
            .map(|i| BinPresentation {
                group_ref: format!("g{i}"),
                display_name: format!("bin {i}"),
                legend_class_key: "c".into(),
                display_flags: vec![],
            })
            .collect(),
        item_presentation: (1..=4096)
            .map(|n| RowPresentation {
                item: ItemRef {
                    result_version: id(2),
                    item_no: n,
                },
                legend_class_key: "c".into(),
                display_flags: vec![],
            })
            .collect(),
    };
    let mut checks = 0;
    assert!(matches!(
        histogram::prepare_selected(request(), compute()?, &mut || {
            checks += 1;
            checks > 1000
        }),
        Err(histogram::Error::Cancelled)
    ));
    let candidate = histogram::prepare_selected(request(), selected, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        candidate.model().histogram.bins[0].members.as_ptr(),
        member_ptr
    );
    assert_eq!(candidate.pages().groups["g0"].rows.len(), 4096);
    Ok(())
}
