use du_analysis_index::visualization::{registry, squarify};
use du_core_contracts::visualization::{LayoutDescriptor, LayoutStrategyKey};

fn descriptor(format: &str, id: &str, revision: u32) -> LayoutDescriptor {
    LayoutDescriptor {
        format: format.into(),
        strategy: LayoutStrategyKey {
            id: id.into(),
            revision,
        },
    }
}

#[test]
fn a_new_format_and_revision_preserve_explicit_dispatch_to_the_original_layout()
-> Result<(), String> {
    type Layout =
        fn(squarify::Rect, &[squarify::Weight]) -> Result<squarify::Layout, squarify::LayoutError>;
    let first = descriptor("treemap", "viz.treemap", 1);
    let next = descriptor("treemap", "viz.treemap", 2);
    let extension = descriptor("fixture_chart", "fixture.layout", 1);
    let mut builder = registry::Builder::<Layout>::default();
    builder
        .register(first.clone(), squarify::layout)
        .map_err(|e| format!("{e:?}"))?;
    builder
        .register(next.clone(), |_, _| {
            Err(squarify::LayoutError::NotDescending)
        })
        .map_err(|e| format!("{e:?}"))?;
    builder
        .register(extension.clone(), |_, _| {
            Err(squarify::LayoutError::InvalidBounds)
        })
        .map_err(|e| format!("{e:?}"))?;
    let registry = builder
        .build(std::slice::from_ref(&first))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        registry.catalog().collect::<Vec<_>>(),
        [&extension, &first, &next]
    );
    let parent = squarify::Rect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
    };
    let weights = [squarify::Weight::Exact(3), squarify::Weight::Exact(2)];
    let original = registry
        .resolve("treemap", &first.strategy)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (original.implementation)(parent, &weights),
        squarify::layout(parent, &weights)
    );
    let newer = registry
        .resolve("treemap", &next.strategy)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (newer.implementation)(parent, &weights),
        Err(squarify::LayoutError::NotDescending)
    );
    assert_eq!(
        registry.latest("treemap").map(|entry| &entry.descriptor),
        Ok(&next)
    );
    assert!(matches!(
        registry.latest("missing"),
        Err(registry::SelectionError::UnknownFormat)
    ));
    assert!(matches!(
        registry.resolve("treemap", &extension.strategy),
        Err(registry::SelectionError::UnknownStrategy)
    ));
    assert!(matches!(
        registry.resolve("unsupported_chart", &first.strategy),
        Err(registry::SelectionError::UnknownFormat)
    ));
    Ok(())
}

#[test]
fn composition_rejects_duplicate_invalid_and_missing_required_entries() -> Result<(), String> {
    let first = descriptor("treemap", "viz.treemap", 1);
    let mut builder = registry::Builder::default();
    builder
        .register(first.clone(), 1)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        builder.register(descriptor("other", "viz.treemap", 1), 2),
        Err(registry::RegistrationError::DuplicateStrategy)
    );
    assert_eq!(
        builder.register(descriptor("other", "viz.other", 0), 2),
        Err(registry::RegistrationError::InvalidDescriptor)
    );
    assert_eq!(
        builder.register(descriptor("treemap", "viz.unrelated", 1), 2),
        Err(registry::RegistrationError::ConflictingFormat)
    );
    assert_eq!(
        builder.register(descriptor("other", "viz.treemap", 2), 2),
        Err(registry::RegistrationError::ConflictingFormat)
    );
    assert!(matches!(
        builder.build(&[first, descriptor("bar", "viz.bar", 1)]),
        Err(registry::RegistrationError::RequiredStrategyMissing)
    ));
    Ok(())
}
