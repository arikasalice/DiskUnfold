use du_analysis_index::visualization::{
    bundle, registry,
    typed_layout::{self, Dispatcher, Input, Strategy},
};
use du_core_contracts::visualization::{LayoutDescriptor, LayoutStrategyKey};
use du_engine_api::viz_catalog::{self, Error};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct NeverCalled(Arc<AtomicUsize>);
impl Strategy for NeverCalled {
    fn layout(
        &self,
        _: &Input,
        _: &mut dyn FnMut() -> bool,
    ) -> Result<typed_layout::Output, typed_layout::Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(typed_layout::Error::InvalidInput)
    }
}
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
fn catalog_uses_registered_open_formats_and_selects_without_execution() -> Result<(), String> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    let entries = [
        descriptor("treemap", "viz.treemap", 1),
        descriptor("treemap", "viz.treemap", 2),
        descriptor("future", "viz.future", 1),
    ];
    for entry in &entries {
        builder
            .register(entry.clone(), Box::new(NeverCalled(calls.clone())))
            .map_err(|e| format!("{e:?}"))?;
    }
    let dispatcher = Dispatcher::new(builder.build(&entries).map_err(|e| format!("{e:?}"))?);
    let result = viz_catalog::snapshot(&dispatcher).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.formats, ["future", "treemap"]);
    assert_eq!(
        result.strategies,
        [entries[2].clone(), entries[0].clone(), entries[1].clone()]
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "treemap", None),
        Ok(entries[1].clone())
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "treemap", Some(&entries[0].strategy)),
        Ok(entries[0].clone())
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "future", None),
        Ok(entries[2].clone())
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "unknown", None),
        Err(Error::UnknownFormat)
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "treemap", Some(&entries[2].strategy)),
        Err(Error::UnknownStrategy)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn empty_registry_is_not_a_successful_catalog() -> Result<(), String> {
    let builder = registry::Builder::<Box<dyn Strategy>>::default();
    let dispatcher = Dispatcher::new(builder.build(&[]).map_err(|e| format!("{e:?}"))?);
    assert_eq!(
        viz_catalog::snapshot(&dispatcher),
        Err(Error::EmptyRegistry)
    );
    assert_eq!(
        viz_catalog::select(&dispatcher, "treemap", None),
        Err(Error::UnknownFormat)
    );
    Ok(())
}

#[test]
fn development_catalog_reports_the_actual_registered_bundle() -> Result<(), String> {
    let dispatcher = bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?;
    let result = viz_catalog::snapshot(&dispatcher).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.formats, ["treemap"]);
    assert_eq!(result.strategies, [typed_layout::treemap_descriptor()]);
    Ok(())
}
