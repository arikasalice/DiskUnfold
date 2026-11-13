use super::{
    Dispatcher, Error as LayoutError, Input, Output, Shape, Strategy, Treemap, treemap_descriptor,
};
use crate::{
    IngestSink, Observation, ResultVersion,
    visualization::{
        geometry::Rect,
        projection::{self, Options, Source},
        registry,
        treemap::ChildKey,
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
use std::sync::Arc;

struct Extension(u32);
impl Strategy for Extension {
    fn layout(
        &self,
        input: &Input,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Output, LayoutError> {
        let mut shapes = Vec::new();
        for (row_index, _) in input.rows().iter().enumerate() {
            if cancelled() {
                return Err(LayoutError::Cancelled);
            }
            shapes.push(Shape {
                row_index,
                geometry: Geometry::new(
                    "point",
                    vec![Point {
                        x: self.0,
                        y: row_index as u32,
                    }],
                )
                .map_err(LayoutError::Geometry)?,
            });
        }
        Ok(Output {
            shapes,
            skipped: vec![],
        })
    }
}
fn descriptor(revision: u32) -> LayoutDescriptor {
    LayoutDescriptor {
        format: "fixture.future".into(),
        strategy: LayoutStrategyKey {
            id: "fixture.layout".into(),
            revision,
        },
    }
}
fn source() -> Result<(Arc<ResultVersion>, ItemRef, Vec<ChildKey>), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open(Id::parse("018f0000-0000-7000-8000-000000000001")?)
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, bytes) in [(b"a".as_slice(), 5_u64), (b"b".as_slice(), 8)] {
        build
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
    let version = build
        .seal(
            Id::parse("018f0000-0000-7000-8000-000000000002")?,
            ResultCompleteness::Complete,
        )
        .map_err(|e| format!("{e:?}"))?;
    let reference = |item_no| ItemRef {
        result_version: version.version_id().clone(),
        item_no,
    };
    let keys = version
        .items()
        .iter()
        .filter(|item| item.parent == Some(0))
        .map(|item| ChildKey {
            item: reference(item.item_no),
            path_compare_key: item.name.clone(),
        })
        .collect();
    Ok((version.clone(), reference(0), keys))
}
fn project(
    version: &ResultVersion,
    parent: &ItemRef,
    keys: &[ChildKey],
    dispatcher: &Dispatcher,
    format: &str,
    strategy: Option<&LayoutStrategyKey>,
) -> Result<projection::Model, projection::Error> {
    projection::project(
        Source {
            version,
            parent,
            basis: SizeBasis::Logical,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            keys,
        },
        dispatcher,
        Options {
            format,
            strategy,
            max_shapes: 10,
        },
        &mut || false,
    )
}

#[test]
fn frozen_catalog_selects_extension_and_explicit_versions_into_common_model() -> Result<(), String>
{
    let first = treemap_descriptor();
    let v1 = descriptor(1);
    let v2 = descriptor(2);
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(first.clone(), Box::new(Treemap))
        .map_err(|e| format!("{e:?}"))?;
    builder
        .register(v2.clone(), Box::new(Extension(20)))
        .map_err(|e| format!("{e:?}"))?;
    builder
        .register(v1.clone(), Box::new(Extension(10)))
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(
        builder
            .build(std::slice::from_ref(&first))
            .map_err(|e| format!("{e:?}"))?,
    );
    let catalog: Vec<_> = dispatcher.catalog().cloned().collect();
    assert_eq!(catalog, vec![v1.clone(), v2.clone(), first.clone()]);
    let (version, parent, keys) = source()?;
    let newest = project(&version, &parent, &keys, &dispatcher, &v1.format, None)
        .map_err(|e| format!("{e:?}"))?;
    let original = project(
        &version,
        &parent,
        &keys,
        &dispatcher,
        &v1.format,
        Some(&v1.strategy),
    )
    .map_err(|e| format!("{e:?}"))?;
    let tree = project(
        &version,
        &parent,
        &keys,
        &dispatcher,
        &first.format,
        Some(&first.strategy),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(newest.strategy, v2);
    assert_eq!(original.strategy, v1);
    assert_eq!(tree.strategy, first);
    assert_resolve_matches(
        &dispatcher,
        &newest.strategy,
        &original.strategy,
        &tree.strategy,
    );
    assert_eq!(newest.rows, original.rows);
    assert_eq!(newest.rows, tree.rows);
    assert_eq!(newest.shapes[0].geometry.points()[0].x, 20);
    assert_eq!(original.shapes[0].geometry.points()[0].x, 10);
    assert_eq!(newest.shapes[0].geometry.primitive(), "point");
    assert_eq!(tree.shapes[0].geometry.primitive(), "rect");
    for shape in &newest.shapes {
        assert_eq!(
            newest.rows[shape.row_index].item.result_version,
            *version.version_id()
        );
    }
    assert!(matches!(
        project(&version, &parent, &keys, &dispatcher, "unknown", None),
        Err(projection::Error::Dispatch(
            super::DispatchError::Selection(registry::SelectionError::UnknownFormat)
        ))
    ));
    assert!(matches!(
        project(
            &version,
            &parent,
            &keys,
            &dispatcher,
            &v1.format,
            Some(&descriptor(9).strategy)
        ),
        Err(projection::Error::Dispatch(
            super::DispatchError::Selection(registry::SelectionError::UnknownStrategy)
        ))
    ));
    Ok(())
}

fn assert_resolve_matches(
    dispatcher: &Dispatcher,
    newest: &LayoutDescriptor,
    original: &LayoutDescriptor,
    tree: &LayoutDescriptor,
) {
    assert_eq!(dispatcher.resolve(&original.format, None), Ok(newest));
    assert_eq!(
        dispatcher.resolve(&original.format, Some(&original.strategy)),
        Ok(original)
    );
    assert_eq!(
        dispatcher.resolve(&tree.format, Some(&tree.strategy)),
        Ok(tree)
    );
    assert_eq!(
        dispatcher.resolve("unknown", None),
        Err(registry::SelectionError::UnknownFormat)
    );
    assert_eq!(
        dispatcher.resolve(&original.format, Some(&descriptor(9).strategy)),
        Err(registry::SelectionError::UnknownStrategy)
    );
}

/// Records invocation while preserving a strategy's behavior.
struct Counting<S>(S, Arc<std::sync::atomic::AtomicUsize>);
impl<S: Strategy> Strategy for Counting<S> {
    fn layout(
        &self,
        input: &Input,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Output, LayoutError> {
        self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.0.layout(input, cancelled)
    }
}
#[test]
fn selection_failures_never_invoke_a_registered_strategy() -> Result<(), String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let first = treemap_descriptor();
    let extension = descriptor(1);
    let treemap_calls = Arc::new(AtomicUsize::new(0));
    let extension_calls = Arc::new(AtomicUsize::new(0));
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(
            first.clone(),
            Box::new(Counting(Treemap, treemap_calls.clone())),
        )
        .map_err(|e| format!("{e:?}"))?;
    builder
        .register(
            extension.clone(),
            Box::new(Counting(Extension(7), extension_calls.clone())),
        )
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(builder.build(&[first]).map_err(|e| format!("{e:?}"))?);
    let (version, parent, keys) = source()?;
    assert_eq!(dispatcher.resolve(&extension.format, None), Ok(&extension));
    assert_eq!(treemap_calls.load(Ordering::SeqCst), 0);
    assert_eq!(extension_calls.load(Ordering::SeqCst), 0);
    let unknown_format = project(
        &version,
        &parent,
        &keys,
        &dispatcher,
        "fixture.missing",
        None,
    );
    assert!(matches!(
        unknown_format,
        Err(projection::Error::Dispatch(
            super::DispatchError::Selection(registry::SelectionError::UnknownFormat)
        ))
    ));
    let unknown_revision = project(
        &version,
        &parent,
        &keys,
        &dispatcher,
        &extension.format,
        Some(&descriptor(99).strategy),
    );
    assert!(matches!(
        unknown_revision,
        Err(projection::Error::Dispatch(
            super::DispatchError::Selection(registry::SelectionError::UnknownStrategy)
        ))
    ));
    assert_eq!(treemap_calls.load(Ordering::SeqCst), 0);
    assert_eq!(extension_calls.load(Ordering::SeqCst), 0);
    let output = project(
        &version,
        &parent,
        &keys,
        &dispatcher,
        &extension.format,
        None,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(output.strategy, extension);
    assert_eq!(output.shapes[0].geometry.primitive(), "point");
    assert_eq!(treemap_calls.load(Ordering::SeqCst), 0);
    assert_eq!(extension_calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn legacy_adapter_borrows_rows_without_allocating_common_rows() -> Result<(), String> {
    let (version, parent, keys) = source()?;
    let input = Input::admit(
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
    )
    .map_err(|e| format!("{e:?}"))?;
    let adapted = super::CommonInput::from_legacy(&input);
    assert!(adapted.is_legacy());
    assert_eq!(adapted.rows.len(), 0);
    assert_eq!(adapted.rows.capacity(), 0);
    assert_eq!(adapted.row_count(), input.rows().len());
    assert!(std::ptr::eq(
        adapted.legacy.ok_or("missing borrowed source")?,
        &input
    ));
    Ok(())
}

#[test]
fn histogram_rejects_legacy_input_instead_of_drawing_an_empty_result() -> Result<(), String> {
    let (version, parent, keys) = source()?;
    let input = Input::admit(
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
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    crate::visualization::bundle::register_histogram_candidate(&mut builder)
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(builder.build(&[]).map_err(|e| format!("{e:?}"))?);
    assert!(matches!(
        dispatcher.execute("histogram", None, &input, &mut || false),
        Err(super::DispatchError::Layout(LayoutError::InvalidInput))
    ));
    Ok(())
}
