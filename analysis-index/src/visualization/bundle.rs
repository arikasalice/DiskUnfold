//! Composition of implemented typed visualization layout strategies.
//!
//! The current typed boundary admits only an exact single-parent treemap input.
//! A partial bundle is not a release-ready F06 default bundle or catalog.
use super::{
    registry,
    typed_layout::{self, Dispatcher, Strategy, Treemap},
};

/// Baseline strategies beyond the implemented treemap; each requires revision one.
const OTHER_REQUIRED_IDS: [&str; 5] = [
    "viz.bar",
    "viz.trend",
    "viz.waterfall",
    "viz.histogram",
    "viz.sankey",
];

/// Composition failure; no partial dispatcher is returned for a full bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Registration(registry::RegistrationError),
    MissingRequired(&'static str),
}

/// Adds only currently implemented typed strategies to a caller-owned builder.
///
/// Duplicate registration is an error. This does not assert that the full F06
/// default bundle is implemented, and must not be used to advertise all formats.
pub fn register_implemented(
    builder: &mut registry::Builder<Box<dyn Strategy>>,
) -> Result<(), Error> {
    builder
        .register(typed_layout::treemap_descriptor(), Box::new(Treemap))
        .map_err(Error::Registration)
}

/// Constructs the currently available single-parent treemap dispatcher.
///
/// This is an explicitly partial development composition; app startup must use
/// `finish_default` to enforce the complete published baseline.
pub fn implemented_dispatcher() -> Result<Dispatcher, Error> {
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    register_implemented(&mut builder)?;
    let registry = builder
        .build(&[typed_layout::treemap_descriptor()])
        .map_err(Error::Registration)?;
    Ok(Dispatcher::new(registry))
}

/// Freezes a full baseline only when all six published IDs at revision one exist.
///
/// The builder still enforces unique IDs, revisions, and format associations.
/// Composition owns the implementations and their correct format keys; this
/// function does not manufacture strategies or register a public use case.
pub fn finish_default(builder: registry::Builder<Box<dyn Strategy>>) -> Result<Dispatcher, Error> {
    let registry = builder.build(&[]).map_err(Error::Registration)?;
    let treemap = typed_layout::treemap_descriptor();
    if registry
        .resolve(&treemap.format, &treemap.strategy)
        .is_err()
    {
        return Err(Error::MissingRequired("viz.treemap"));
    }
    for id in OTHER_REQUIRED_IDS {
        if !registry
            .catalog()
            .any(|entry| entry.strategy.id == id && entry.strategy.revision == 1)
        {
            return Err(Error::MissingRequired(id));
        }
    }
    Ok(Dispatcher::new(registry))
}
