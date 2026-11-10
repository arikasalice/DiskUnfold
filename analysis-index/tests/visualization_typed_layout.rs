use du_analysis_index::{
    IngestSink, Observation,
    visualization::{
        geometry::{NoArea, Rect},
        registry, treemap,
        typed_layout::{self, Input, Output, Shape, Strategy},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
    visualization::{
        LayoutDescriptor, LayoutStrategyKey,
        geometry::{Geometry, Point},
    },
};
fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn descriptor(format: &str, key: &str, revision: u32) -> LayoutDescriptor {
    LayoutDescriptor {
        format: format.into(),
        strategy: LayoutStrategyKey {
            id: key.into(),
            revision,
        },
    }
}
fn prepared() -> Result<Input, String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, n) in [
        (b"a".as_slice(), 10),
        (b"b".as_slice(), 10),
        (b"zero".as_slice(), 0),
    ] {
        b.add_file(Observation {
            path: vec![name.to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact { bytes: n.into() },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    let v = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let parent = ItemRef {
        result_version: v.version_id().clone(),
        item_no: 0,
    };
    let keys: Vec<_> = v
        .items()
        .iter()
        .filter(|i| i.parent == Some(0))
        .map(|i| treemap::ChildKey {
            item: ItemRef {
                result_version: v.version_id().clone(),
                item_no: i.item_no,
            },
            path_compare_key: if i.name == b"a" {
                b"z".to_vec()
            } else {
                i.name.clone()
            },
        })
        .collect();
    Input::admit(
        &v,
        &parent,
        SizeBasis::Logical,
        Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        &keys,
    )
    .map_err(|e| format!("{e:?}"))
}
struct Fixture;
impl Strategy for Fixture {
    fn layout(
        &self,
        input: &Input,
        _: &mut dyn FnMut() -> bool,
    ) -> Result<Output, typed_layout::Error> {
        let geometry = Geometry::new("point", vec![Point { x: 1, y: 2 }])
            .map_err(typed_layout::Error::Geometry)?;
        Ok(Output {
            shapes: input
                .rows()
                .iter()
                .enumerate()
                .map(|(row_index, _)| Shape {
                    row_index,
                    geometry: geometry.clone(),
                })
                .collect(),
            skipped: vec![],
        })
    }
}
struct Invalid;
impl Strategy for Invalid {
    fn layout(
        &self,
        _: &Input,
        _: &mut dyn FnMut() -> bool,
    ) -> Result<Output, typed_layout::Error> {
        let geometry = Geometry::new("point", vec![Point { x: 0, y: 0 }])
            .map_err(typed_layout::Error::Geometry)?;
        Ok(Output {
            shapes: vec![Shape {
                row_index: usize::MAX,
                geometry,
            }],
            skipped: vec![],
        })
    }
}
#[test]
fn one_pinned_input_has_homogeneous_treemap_and_extension_outputs() -> Result<(), String> {
    let input = prepared()?;
    assert_eq!(input.rows().len(), 3);
    assert_eq!(input.rows()[0].path_compare_key(), b"b");
    assert_eq!(input.rows()[1].path_compare_key(), b"z");
    assert_eq!(input.rows()[0].value().evidence, SizeEvidence::Measured);
    let original = typed_layout::treemap_descriptor();
    let fixture = descriptor("fixture", "fixture.test", 1);
    let invalid = descriptor("bad", "fixture.bad", 1);
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(original.clone(), Box::new(typed_layout::Treemap))
        .map_err(|e| format!("{e:?}"))?;
    builder
        .register(fixture.clone(), Box::new(Fixture))
        .map_err(|e| format!("{e:?}"))?;
    builder
        .register(invalid.clone(), Box::new(Invalid))
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher = typed_layout::Dispatcher::new(
        builder
            .build(std::slice::from_ref(&original))
            .map_err(|e| format!("{e:?}"))?,
    );
    assert_eq!(dispatcher.catalog().count(), 3);
    let live = dispatcher
        .execute("treemap", Some(&original.strategy), &input, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(live.descriptor, original);
    assert_eq!(live.output.shapes.len(), 2);
    assert_eq!(live.output.skipped[0].reason, NoArea::ExactZero);
    for shape in &live.output.shapes {
        assert_eq!(shape.geometry.primitive(), "rect");
        assert!(shape.row_index < input.rows().len());
    }
    let other = dispatcher
        .execute("fixture", None, &input, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(other.descriptor, fixture);
    assert_eq!(other.output.shapes.len(), 3);
    assert!(
        other
            .output
            .shapes
            .iter()
            .all(|s| s.geometry.primitive() == "point")
    );
    assert_eq!(
        dispatcher.execute("bad", None, &input, &mut || false).err(),
        Some(typed_layout::DispatchError::Layout(
            typed_layout::Error::InvalidReference
        ))
    );
    assert_eq!(
        dispatcher
            .execute("treemap", None, &input, &mut || true)
            .err(),
        Some(typed_layout::DispatchError::Layout(
            typed_layout::Error::Cancelled
        ))
    );
    assert_eq!(
        dispatcher
            .execute(
                "treemap",
                Some(&descriptor("treemap", "viz.treemap", 9).strategy),
                &input,
                &mut || false
            )
            .err(),
        Some(typed_layout::DispatchError::Selection(
            registry::SelectionError::UnknownStrategy
        ))
    );
    Ok(())
}

#[test]
fn unproven_values_are_not_promoted_to_layout_input() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    b.add_file(Observation {
        path: vec![b"missing".to_vec()],
        size: None,
    })
    .map_err(|e| format!("{e:?}"))?;
    let v = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let reference = |item_no| ItemRef {
        result_version: v.version_id().clone(),
        item_no,
    };
    let key = treemap::ChildKey {
        item: reference(1),
        path_compare_key: b"missing".to_vec(),
    };
    assert!(matches!(
        Input::admit(
            &v,
            &reference(0),
            SizeBasis::Logical,
            Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100
            },
            &[key]
        ),
        Err(treemap::AdmissionError::NoAttribution)
    ));
    Ok(())
}

#[test]
fn cancellation_is_observed_during_admission_and_before_return() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for n in 0..40u8 {
        b.add_file(Observation {
            path: vec![vec![n + 1]],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: u64::from(n + 1).into(),
                },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    let version = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let reference = |item_no| ItemRef {
        result_version: version.version_id().clone(),
        item_no,
    };
    let keys: Vec<_> = version
        .items()
        .iter()
        .filter(|i| i.parent == Some(0))
        .map(|item| treemap::ChildKey {
            item: reference(item.item_no),
            path_compare_key: item.name.clone(),
        })
        .collect();
    let parent = reference(0);
    let mut total = 0;
    treemap::admit_cancellable(&version, &parent, SizeBasis::Logical, &keys, &mut || {
        total += 1;
        false
    })
    .map_err(|e| format!("{e:?}"))?;
    for stop in [1, 50, 140, total - 2, total] {
        let mut calls = 0;
        let result =
            treemap::admit_cancellable(&version, &parent, SizeBasis::Logical, &keys, &mut || {
                calls += 1;
                calls == stop
            });
        assert!(
            matches!(result, Err(treemap::AdmissionError::Cancelled)),
            "stop={stop}, total={total}"
        );
        assert_eq!(calls, stop);
    }
    let mut calls = 0;
    let result = Input::admit_cancellable(
        &version,
        &parent,
        SizeBasis::Logical,
        Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        &keys,
        &mut || {
            calls += 1;
            calls == 80
        },
    );
    assert!(matches!(result, Err(treemap::AdmissionError::Cancelled)));
    Ok(())
}

#[test]
fn dispatcher_discards_a_result_if_cancellation_arrives_at_return() -> Result<(), String> {
    let input = prepared()?;
    let fixture = descriptor("fixture", "fixture.test", 1);
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(fixture.clone(), Box::new(Fixture))
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher =
        typed_layout::Dispatcher::new(builder.build(&[fixture]).map_err(|e| format!("{e:?}"))?);
    assert_eq!(
        dispatcher
            .execute("fixture", None, &input, &mut || true)
            .err(),
        Some(typed_layout::DispatchError::Layout(
            typed_layout::Error::Cancelled
        ))
    );
    Ok(())
}

#[test]
fn admitted_strategy_matches_existing_layout_parent_geometry() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, bytes) in [
        (b"a".as_slice(), 8),
        (b"b".as_slice(), 8),
        (b"c".as_slice(), 0),
    ] {
        b.add_file(Observation {
            path: vec![name.to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: bytes.into(),
                },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    let v = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let reference = |item_no| ItemRef {
        result_version: v.version_id().clone(),
        item_no,
    };
    let keys: Vec<_> = v
        .items()
        .iter()
        .filter(|i| i.parent == Some(0))
        .map(|i| treemap::ChildKey {
            item: reference(i.item_no),
            path_compare_key: if i.name == b"a" {
                b"z".to_vec()
            } else {
                i.name.clone()
            },
        })
        .collect();
    let bounds = Rect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
    };
    let old = treemap::layout_parent(&v, &reference(0), SizeBasis::Logical, bounds, &keys)
        .map_err(|e| format!("{e:?}"))?;
    let input = Input::admit(&v, &reference(0), SizeBasis::Logical, bounds, &keys)
        .map_err(|e| format!("{e:?}"))?;
    let actual = typed_layout::Treemap
        .layout(&input, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let expected = old.layout.geometry_v1().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        actual
            .shapes
            .iter()
            .map(|shape| (shape.row_index, &shape.geometry))
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|(index, geometry)| (*index, geometry))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        input
            .rows()
            .iter()
            .map(|row| row.item())
            .collect::<Vec<_>>(),
        old.children.iter().map(|row| &row.item).collect::<Vec<_>>()
    );
    assert_eq!(actual.skipped.len(), old.layout.skipped.len());
    Ok(())
}

#[test]
fn treemap_propagates_cancellation_inside_squarify() -> Result<(), String> {
    let input = prepared()?;
    let mut checks = 0;
    let result = typed_layout::Treemap.layout(&input, &mut || {
        checks += 1;
        // The outer preflight and weight-copy checks finish first; this is
        // during squarify's own input traversal.
        checks == input.rows().len() + 3
    });
    assert_eq!(result, Err(typed_layout::Error::Cancelled));
    assert_eq!(checks, input.rows().len() + 3);
    Ok(())
}
