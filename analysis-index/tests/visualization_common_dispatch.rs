use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    visualization::{
        bundle,
        geometry::{Rect, Scale},
        histogram, histogram_dispatch, registry,
        typed_layout::{CommonInput, Dispatcher, Error, Output, Shape, Strategy},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::ResultCompleteness,
    size_units::{IEC_UNITS, SI_UNITS, UNIT_TABLE_REVISION, UnitTable},
    visualization::{
        LayoutDescriptor, LayoutStrategyKey,
        geometry::{Geometry, Point},
        quantity::VizAmount,
    },
};
use std::sync::Arc;

fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}
fn publish(
    sink: &IngestSink,
    version: u8,
    values: &[Option<u64>],
) -> Result<Arc<ResultVersion>, String> {
    let mut b = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (i, n) in values.iter().enumerate() {
        b.add_file(Observation {
            path: vec![format!("{i}").into_bytes()],
            size: n.map(|v| SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact { bytes: v.into() },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    b.seal(id(version), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))
}
fn prep<'a>(
    v: &'a ResultVersion,
    scope: &'a ItemRef,
    table: &'a UnitTable,
) -> histogram_dispatch::Request<'a> {
    histogram_dispatch::Request {
        version: v,
        scope,
        size_basis: SizeBasis::Logical,
        unit_table: table,
        unit_table_revision: UNIT_TABLE_REVISION,
        bounds: Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        basis: histogram::Basis::ItemCount,
        scale: Scale::Fixed(20),
        unknown_reason: "fixture.unknown",
    }
}
struct Extension(u32);
impl Strategy for Extension {
    fn layout_common(
        &self,
        input: &CommonInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Output, Error> {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut shapes = Vec::new();
        if !input.rows.is_empty() {
            shapes.push(Shape {
                row_index: 0,
                geometry: Geometry::new("point", vec![Point { x: self.0, y: 0 }])
                    .map_err(Error::Geometry)?,
            });
        }
        Ok(Output {
            shapes,
            skipped: vec![],
        })
    }
}
fn ext(revision: u32) -> LayoutDescriptor {
    LayoutDescriptor {
        format: "fixture.format".into(),
        strategy: LayoutStrategyKey {
            id: "fixture.strategy".into(),
            revision,
        },
    }
}
fn dispatcher() -> Result<Dispatcher, String> {
    let mut b = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_implemented(&mut b).map_err(|e| format!("{e:?}"))?;
    bundle::register_histogram_candidate(&mut b).map_err(|e| format!("{e:?}"))?;
    b.register(ext(1), Box::new(Extension(1)))
        .map_err(|e| format!("{e:?}"))?;
    b.register(ext(2), Box::new(Extension(2)))
        .map_err(|e| format!("{e:?}"))?;
    Ok(Dispatcher::new(b.build(&[]).map_err(|e| format!("{e:?}"))?))
}
#[test]
fn histogram_and_extension_use_the_same_common_dispatch() -> Result<(), String> {
    let sink = IngestSink::default();
    let v = publish(&sink, 2, &[Some(0), Some(999), Some(1000), None])?;
    let scope = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let prepared = histogram_dispatch::prepare(prep(&v, &scope, &SI_UNITS), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(prepared.model().summary.result_version, id(2));
    assert_eq!(prepared.model().histogram.bins[0].members.len(), 2);
    assert_eq!(prepared.model().histogram.bins[1].members.len(), 1);
    assert_eq!(prepared.model().histogram.uncertain_items.len(), 1);
    assert!(matches!(
        prepared.input().rows[0].quantity.amount(),
        VizAmount::LowerBound { .. }
    ));
    let d = dispatcher()?;
    let h = d
        .execute_common("histogram", None, prepared.input(), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(h.descriptor, histogram_dispatch::histogram_descriptor());
    assert!(
        h.output
            .shapes
            .iter()
            .all(|s| s.geometry.primitive() == "rect")
    );
    let latest = d
        .execute_common("fixture.format", None, prepared.input(), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let old = d
        .execute_common(
            "fixture.format",
            Some(&ext(1).strategy),
            prepared.input(),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(latest.descriptor, ext(2));
    assert_eq!(old.descriptor, ext(1));
    assert_eq!(latest.output.shapes[0].geometry.points()[0].x, 2);
    assert_eq!(old.output.shapes[0].geometry.points()[0].x, 1);
    assert!(
        d.execute_common(
            "fixture.format",
            Some(&ext(9).strategy),
            prepared.input(),
            &mut || false
        )
        .is_err()
    );
    assert!(
        d.execute_common("histogram", None, prepared.input(), &mut || true)
            .is_err()
    );
    Ok(())
}

#[test]
fn selected_histogram_rejects_wrong_format_and_unregistered_revision() -> Result<(), String> {
    let sink = IngestSink::default();
    let v = publish(&sink, 2, &[Some(10)])?;
    let scope = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let dispatcher = dispatcher()?;
    let make = || {
        histogram_dispatch::prepare(prep(&v, &scope, &SI_UNITS), &mut || false)
            .map_err(|e| format!("{e:?}"))
    };
    assert!(matches!(
        make()?.render_selected(&dispatcher, &ext(1), &mut || false),
        Err(
            du_analysis_index::visualization::typed_layout::DispatchError::Layout(
                Error::InvalidInput
            )
        )
    ));
    let mut unknown = histogram_dispatch::histogram_descriptor();
    unknown.strategy.revision = 99;
    assert!(matches!(
        make()?.render_selected(&dispatcher, &unknown, &mut || false),
        Err(
            du_analysis_index::visualization::typed_layout::DispatchError::Selection(
                registry::SelectionError::UnknownStrategy
            )
        )
    ));
    Ok(())
}
#[test]
fn pinned_result_and_unit_selection_survive_replacement() -> Result<(), String> {
    let sink = IngestSink::default();
    let old = publish(&sink, 2, &[Some(1000), Some(1024)])?;
    let scope = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let si = histogram_dispatch::prepare(prep(&old, &scope, &SI_UNITS), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let iec = histogram_dispatch::prepare(prep(&old, &scope, &IEC_UNITS), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(si.unit_bins().bins[0].upper, Some(1000));
    assert_eq!(iec.unit_bins().bins[0].upper, Some(1024));
    assert_eq!(si.model().histogram.bins[0].members.len(), 0);
    assert_eq!(iec.model().histogram.bins[0].members.len(), 1);
    let new = publish(&sink, 3, &[Some(5)])?;
    assert!(histogram_dispatch::prepare(prep(&new, &scope, &SI_UNITS), &mut || false).is_err());
    assert_eq!(si.model().summary.result_version, id(2));
    assert_eq!(si.model().histogram.bins[1].members.len(), 2);
    let mut bad = prep(&old, &scope, &SI_UNITS);
    bad.unit_table_revision += 1;
    assert!(matches!(
        histogram_dispatch::prepare(bad, &mut || false),
        Err(histogram_dispatch::Error::UnitTable(_))
    ));
    assert!(matches!(
        histogram_dispatch::prepare(prep(&old, &scope, &SI_UNITS), &mut || true),
        Err(histogram_dispatch::Error::Cancelled)
    ));
    assert_eq!(
        dispatcher()?
            .execute_common("histogram", None, si.input(), &mut || false)
            .map_err(|e| format!("{e:?}"))?
            .descriptor,
        histogram_dispatch::histogram_descriptor()
    );
    Ok(())
}

#[test]
fn owner_keeps_ambiguous_and_open_bin_lower_bounds() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, bytes) in [
        (b"ambiguous".as_slice(), 1000_u64),
        (b"last".as_slice(), 1000_u64.pow(5)),
    ] {
        build
            .add_file(Observation {
                path: vec![name.to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::LowerBound {
                        bytes: bytes.into(),
                    },
                }),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let scope = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let mut request = prep(&version, &scope, &SI_UNITS);
    request.basis = histogram::Basis::Bytes;
    request.scale = Scale::Automatic;
    let prepared =
        histogram_dispatch::prepare(request, &mut || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(prepared.model().histogram.uncertain_items.len(), 1);
    assert_eq!(
        prepared
            .model()
            .histogram
            .bins
            .last()
            .ok_or("last")?
            .members
            .len(),
        1
    );
    assert!(matches!(
        prepared.input().rows.last().ok_or("row")?.quantity.amount(),
        VizAmount::LowerBound { .. }
    ));
    let result = dispatcher()?
        .execute_common("histogram", None, prepared.input(), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        result.descriptor,
        histogram_dispatch::histogram_descriptor()
    );
    Ok(())
}

#[test]
fn resolved_axis_and_bin_references_belong_to_the_same_render() -> Result<(), String> {
    let sink = IngestSink::default();
    let version = publish(&sink, 2, &[Some(0), Some(999), Some(1000), Some(1024)])?;
    let scope = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let dispatcher = dispatcher()?;
    let mut automatic = prep(&version, &scope, &SI_UNITS);
    automatic.scale = Scale::Automatic;
    let auto = histogram_dispatch::prepare(automatic, &mut || false)
        .map_err(|e| format!("{e:?}"))?
        .render(&dispatcher, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(auto.selected().axis_max, Some(2));
    let mut fixed = prep(&version, &scope, &SI_UNITS);
    fixed.scale = Scale::Fixed(4);
    let fixed = histogram_dispatch::prepare(fixed, &mut || false)
        .map_err(|e| format!("{e:?}"))?
        .render(&dispatcher, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fixed.selected().axis_max, Some(4));
    let first_auto = &auto.selected().output.shapes[0];
    let first_fixed = &fixed.selected().output.shapes[0];
    assert_eq!(first_auto.row_index, first_fixed.row_index);
    assert_eq!(
        auto.prepared().model().histogram.bins[first_auto.row_index]
            .members
            .len(),
        2
    );
    assert_eq!(
        fixed.prepared().model().histogram.bins[first_fixed.row_index]
            .members
            .len(),
        2
    );
    assert!(first_auto.geometry.points()[1].x > first_fixed.geometry.points()[1].x);
    let mut invalid = prep(&version, &scope, &SI_UNITS);
    invalid.scale = Scale::Fixed(1);
    let pending =
        histogram_dispatch::prepare(invalid, &mut || false).map_err(|e| format!("{e:?}"))?;
    assert!(pending.render(&dispatcher, &mut || false).is_err());
    let uncertain = publish(&IngestSink::default(), 3, &[Some(1), None])?;
    let uncertain_scope = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let mut unknown_axis = prep(&uncertain, &uncertain_scope, &SI_UNITS);
    unknown_axis.scale = Scale::Automatic;
    let unknown = histogram_dispatch::prepare(unknown_axis, &mut || false)
        .map_err(|e| format!("{e:?}"))?
        .render(&dispatcher, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(unknown.selected().axis_max, None);
    Ok(())
}
