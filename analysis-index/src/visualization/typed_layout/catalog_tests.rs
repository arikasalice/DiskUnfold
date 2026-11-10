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
