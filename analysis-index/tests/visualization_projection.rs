use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    visualization::{
        AreaValue,
        geometry::{NoArea, Rect},
        projection::{self, Options, Source},
        registry, treemap,
        typed_layout::{self, Dispatcher, Input, Strategy},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};

fn fixture() -> Result<
    (
        std::sync::Arc<ResultVersion>,
        ItemRef,
        Vec<treemap::ChildKey>,
        Dispatcher,
    ),
    String,
> {
    let id = |n| Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"));
    let sink = IngestSink::default();
    let mut builder = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    for (name, bytes) in [
        (b"a".as_slice(), 10),
        (b"b".as_slice(), 20),
        (b"zero".as_slice(), 0),
    ] {
        builder
            .add_file(Observation {
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
    let version = builder
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let item = |n| ItemRef {
        result_version: version.version_id().clone(),
        item_no: n,
    };
    let parent = item(0);
    let keys: Vec<_> = version
        .items()
        .iter()
        .filter(|i| i.parent == Some(0))
        .map(|i| treemap::ChildKey {
            item: item(i.item_no),
            path_compare_key: i.name.clone(),
        })
        .collect();
    let descriptor = typed_layout::treemap_descriptor();
    let mut registry = registry::Builder::<Box<dyn Strategy>>::default();
    registry
        .register(descriptor.clone(), Box::new(typed_layout::Treemap))
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(
        registry
            .build(&[descriptor])
            .map_err(|e| format!("{e:?}"))?,
    );
    Ok((version, parent, keys, dispatcher))
}
fn source<'a>(
    version: &'a ResultVersion,
    parent: &'a ItemRef,
    basis: SizeBasis,
    keys: &'a [treemap::ChildKey],
) -> Source<'a> {
    Source {
        version,
        parent,
        basis,
        keys,
        bounds: Rect {
            left: 0,
            top: 0,
            right: 1_000_000,
            bottom: 1_000_000,
        },
    }
}
fn options(limit: usize) -> Options<'static> {
    Options {
        format: "treemap",
        strategy: None,
        max_shapes: limit,
    }
}
#[test]
fn projection_keeps_source_rows_and_verified_totals() -> Result<(), String> {
    let (version, parent, keys, dispatcher) = fixture()?;
    let model = projection::project(
        source(&version, &parent, SizeBasis::Logical, &keys),
        &dispatcher,
        options(10),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(model.rows.len(), 3);
    assert_eq!(
        model
            .rows
            .iter()
            .map(|row| row.ratio.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("0.66666666666666666667"),
            Some("0.33333333333333333333"),
            Some("0")
        ]
    );
    assert_eq!(model.shapes.len(), 2);
    assert_eq!(model.omissions.len(), 1);
    assert_eq!(
        model.totals.scope,
        version.space(0).ok_or("missing scope")?.logical
    );
    assert_eq!(
        (
            model.totals.drawn_bytes,
            model.totals.grouped_bytes,
            model.totals.filtered_bytes
        ),
        (30, 0, 0)
    );
    for shape in &model.shapes {
        let row = &model.rows[shape.row_index];
        let source = version
            .items()
            .get(row.item.item_no as usize)
            .ok_or("missing item")?;
        assert_eq!(row.size, *source.size.as_ref().ok_or("missing size")?);
        assert_eq!(row.item.result_version, *version.version_id());
        assert_eq!(shape.meta.visible_depth, 1);
        assert_eq!(shape.meta.shape_ref, format!("s{:x}-1", row.item.item_no));
        assert_eq!(shape.meta.root_branch_ref, None);
        assert_eq!(row.root_branch_ref, None);
        assert_eq!(row.area_value, AreaValue::Positive);
        assert_eq!(row.omission, None);
        assert_eq!(row.omitted_children, 0);
    }
    let zero = model
        .rows
        .iter()
        .find(|row| row.area_value == AreaValue::ExactZero)
        .ok_or("missing zero")?;
    assert_eq!(zero.omission, Some(NoArea::ExactZero));
    assert_eq!(model.strategy, typed_layout::treemap_descriptor());
    Ok(())
}

#[test]
fn folder_branch_is_carried_to_projection_row_and_shape() -> Result<(), String> {
    let id = |n| Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"));
    let sink = IngestSink::default();
    let mut builder = sink.open(id(10)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![b"folder".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    builder
        .add_file(Observation {
            path: vec![b"folder".to_vec(), b"file".to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: 5_u64.into(),
                },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = builder
        .seal(id(11)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let parent = ItemRef {
        result_version: version.version_id().clone(),
        item_no: 0,
    };
    let folder = ItemRef {
        result_version: version.version_id().clone(),
        item_no: version
            .items()
            .iter()
            .find(|item| item.name == b"folder")
            .ok_or("missing folder")?
            .item_no,
    };
    let keys = vec![treemap::ChildKey {
        item: folder.clone(),
        path_compare_key: b"folder".to_vec(),
    }];
    let (_, _, _, dispatcher) = fixture()?;
    let model = projection::project(
        source(&version, &parent, SizeBasis::Logical, &keys),
        &dispatcher,
        options(10),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(model.rows[0].root_branch_ref, Some(folder.clone()));
    assert_eq!(model.rows[0].omitted_children, 1);
    assert_eq!(model.shapes[0].meta.root_branch_ref, Some(folder));
    Ok(())
}
#[test]
fn limits_wrong_sources_and_cancellation_do_not_expose_partial_models() -> Result<(), String> {
    let (version, parent, keys, dispatcher) = fixture()?;
    assert_eq!(
        projection::project(
            source(&version, &parent, SizeBasis::Logical, &keys),
            &dispatcher,
            options(1),
            &mut || false
        )
        .err(),
        Some(projection::Error::ShapeLimitExceeded)
    );
    assert_eq!(
        projection::project(
            source(&version, &parent, SizeBasis::Allocated, &keys),
            &dispatcher,
            options(10),
            &mut || false
        )
        .err(),
        Some(projection::Error::UnavailableAttribution)
    );
    assert_eq!(
        projection::project(
            source(&version, &parent, SizeBasis::Logical, &keys),
            &dispatcher,
            options(10),
            &mut || true
        )
        .err(),
        Some(projection::Error::Cancelled)
    );
    let mut wrong = parent.clone();
    wrong.item_no = 1;
    assert_eq!(
        projection::project(
            source(&version, &wrong, SizeBasis::Logical, &keys),
            &dispatcher,
            options(10),
            &mut || false
        )
        .err(),
        Some(projection::Error::InvalidSource)
    );
    Ok(())
}

struct Duplicate;
impl Strategy for Duplicate {
    fn layout(
        &self,
        _: &Input,
        _: &mut dyn FnMut() -> bool,
    ) -> Result<typed_layout::Output, typed_layout::Error> {
        let geometry = du_core_contracts::visualization::geometry::Geometry::new(
            "point",
            vec![du_core_contracts::visualization::geometry::Point { x: 1, y: 1 }],
        )
        .map_err(typed_layout::Error::Geometry)?;
        Ok(typed_layout::Output {
            shapes: vec![
                typed_layout::Shape {
                    row_index: 0,
                    geometry: geometry.clone(),
                },
                typed_layout::Shape {
                    row_index: 0,
                    geometry,
                },
            ],
            skipped: vec![],
        })
    }
}
#[test]
fn duplicate_or_missing_row_from_strategy_is_rejected() -> Result<(), String> {
    let (version, parent, keys, _) = fixture()?;
    let descriptor = du_core_contracts::visualization::LayoutDescriptor {
        format: "future".into(),
        strategy: du_core_contracts::visualization::LayoutStrategyKey {
            id: "viz.future".into(),
            revision: 1,
        },
    };
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(descriptor.clone(), Box::new(Duplicate))
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(builder.build(&[descriptor]).map_err(|e| format!("{e:?}"))?);
    let opts = Options {
        format: "future",
        strategy: None,
        max_shapes: 10,
    };
    assert_eq!(
        projection::project(
            source(&version, &parent, SizeBasis::Logical, &keys),
            &dispatcher,
            opts,
            &mut || false
        )
        .err(),
        Some(projection::Error::InvalidLayout)
    );
    Ok(())
}

#[test]
fn rejects_wrong_version_duplicate_keys_and_mid_admission_cancel() -> Result<(), String> {
    let (version, parent, keys, dispatcher) = fixture()?;
    let mut wrong = parent.clone();
    wrong.result_version = Id::parse("018f0000-0000-7000-8000-000000000099")?;
    assert_eq!(
        projection::project(
            source(&version, &wrong, SizeBasis::Logical, &keys),
            &dispatcher,
            options(10),
            &mut || false
        )
        .err(),
        Some(projection::Error::InvalidSource)
    );
    let duplicate = [keys[0].clone(), keys[0].clone()];
    assert_eq!(
        projection::project(
            source(&version, &parent, SizeBasis::Logical, &duplicate),
            &dispatcher,
            options(10),
            &mut || false
        )
        .err(),
        Some(projection::Error::InvalidSource)
    );
    let mut calls = 0;
    assert_eq!(
        projection::project(
            source(&version, &parent, SizeBasis::Logical, &keys),
            &dispatcher,
            options(10),
            &mut || {
                calls += 1;
                calls == 6
            }
        )
        .err(),
        Some(projection::Error::Cancelled)
    );
    assert_eq!(calls, 6);
    Ok(())
}

struct MeasuredStrategy {
    checks: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    at_entry: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl Strategy for MeasuredStrategy {
    fn layout(
        &self,
        input: &Input,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<typed_layout::Output, typed_layout::Error> {
        use std::sync::atomic::Ordering;
        self.at_entry
            .store(self.checks.load(Ordering::Relaxed), Ordering::Relaxed);
        typed_layout::Treemap.layout(input, cancelled)
    }
}
#[test]
fn projection_runs_one_admission_before_strategy() -> Result<(), String> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let (version, parent, keys, _) = fixture()?;
    let src = source(&version, &parent, SizeBasis::Logical, &keys);
    let mut admission_checks = 0;
    Input::admit_cancellable(
        src.version,
        src.parent,
        src.basis.clone(),
        src.bounds,
        src.keys,
        &mut || {
            admission_checks += 1;
            false
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let checks = Arc::new(AtomicUsize::new(0));
    let at_entry = Arc::new(AtomicUsize::new(0));
    let descriptor = typed_layout::treemap_descriptor();
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(
            descriptor.clone(),
            Box::new(MeasuredStrategy {
                checks: checks.clone(),
                at_entry: at_entry.clone(),
            }),
        )
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(builder.build(&[descriptor]).map_err(|e| format!("{e:?}"))?);
    projection::project(src, &dispatcher, options(10), &mut || {
        checks.fetch_add(1, Ordering::Relaxed);
        false
    })
    .map_err(|e| format!("{e:?}"))?;
    // One entry checkpoint belongs to project; the remainder is one admission.
    assert_eq!(at_entry.load(Ordering::Relaxed), admission_checks + 1);
    Ok(())
}

struct DrawZero;
impl Strategy for DrawZero {
    fn layout(
        &self,
        input: &Input,
        _: &mut dyn FnMut() -> bool,
    ) -> Result<typed_layout::Output, typed_layout::Error> {
        let geometry = du_core_contracts::visualization::geometry::Geometry::new(
            "point",
            vec![du_core_contracts::visualization::geometry::Point { x: 1, y: 1 }],
        )
        .map_err(typed_layout::Error::Geometry)?;
        Ok(typed_layout::Output {
            shapes: (0..input.rows().len())
                .map(|row_index| typed_layout::Shape {
                    row_index,
                    geometry: geometry.clone(),
                })
                .collect(),
            skipped: vec![],
        })
    }
}
#[test]
fn rejects_a_strategy_that_draws_exact_zero_with_otherwise_valid_rows() -> Result<(), String> {
    let (version, parent, keys, _) = fixture()?;
    let descriptor = du_core_contracts::visualization::LayoutDescriptor {
        format: "future".into(),
        strategy: du_core_contracts::visualization::LayoutStrategyKey {
            id: "viz.future".into(),
            revision: 1,
        },
    };
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(descriptor.clone(), Box::new(DrawZero))
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(builder.build(&[descriptor]).map_err(|e| format!("{e:?}"))?);
    let opts = Options {
        format: "future",
        strategy: None,
        max_shapes: 10,
    };
    assert_eq!(
        projection::project(
            source(&version, &parent, SizeBasis::Logical, &keys),
            &dispatcher,
            opts,
            &mut || false
        )
        .err(),
        Some(projection::Error::InvalidLayout)
    );
    Ok(())
}
