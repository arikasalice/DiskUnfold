//! Composition of implemented typed visualization layout strategies.
//!
//! The typed registry selects layout implementations. Desktop composition
//! supplies the pinned evidence and exposed formats for each request.
use super::{
    registry,
    typed_layout::{self, Dispatcher, Strategy, Treemap},
};
use du_core_contracts::visualization::LayoutStrategyKey;

/// Baseline format and strategy pairs checked after treemap and bar.
const OTHER_REQUIRED: [(&str, &str); 4] = [
    ("trend", "viz.trend"),
    ("waterfall", "viz.waterfall"),
    ("histogram", "viz.histogram"),
    ("sankey", "viz.sankey"),
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

/// Adds the ranked-only bar candidate to an explicitly partial development registry.
///
/// This does not claim type comparison or stacked support and must not be used
/// as the application catalog until those representations are implemented.
pub fn register_ranked_bar(
    builder: &mut registry::Builder<Box<dyn Strategy>>,
) -> Result<(), Error> {
    builder
        .register(
            typed_layout::bar_descriptor(),
            Box::new(typed_layout::RankedBar),
        )
        .map_err(Error::Registration)
}

/// Registers one bar strategy for ranked, type-comparison and stacked geometry.
/// The runtime must still supply pinned classification and attribution facts.
pub fn register_bar_candidate(
    builder: &mut registry::Builder<Box<dyn Strategy>>,
) -> Result<(), Error> {
    builder
        .register(
            typed_layout::bar_descriptor(),
            Box::new(typed_layout::BarLayout),
        )
        .map_err(Error::Registration)
}

/// Registers a histogram candidate in an explicitly partial dispatcher.
pub fn register_histogram_candidate(
    builder: &mut registry::Builder<Box<dyn Strategy>>,
) -> Result<(), Error> {
    builder
        .register(
            super::histogram_dispatch::histogram_descriptor(),
            Box::new(super::histogram_dispatch::HistogramLayout),
        )
        .map_err(Error::Registration)
}

/// Registers waterfall geometry over an already ordered comparison result.
pub fn register_waterfall_candidate(
    builder: &mut registry::Builder<Box<dyn Strategy>>,
) -> Result<(), Error> {
    builder
        .register(
            super::waterfall_dispatch::descriptor(),
            Box::new(super::waterfall_dispatch::WaterfallLayout),
        )
        .map_err(Error::Registration)
}

/// Registers trend geometry over owner-decimated observations and their timestamps.
pub fn register_trend_candidate(
    builder: &mut registry::Builder<Box<dyn Strategy>>,
) -> Result<(), Error> {
    builder
        .register(
            typed_layout::trend_descriptor(),
            Box::new(typed_layout::TrendLayout),
        )
        .map_err(Error::Registration)
}

/// Registers the Sankey strategy over pinned attribution facts from their owner.
pub fn register_sankey_candidate(
    builder: &mut registry::Builder<Box<dyn Strategy>>,
) -> Result<(), Error> {
    builder
        .register(
            typed_layout::sankey_strategy::descriptor(),
            Box::new(typed_layout::sankey_strategy::SankeyLayout),
        )
        .map_err(Error::Registration)
}

/// Builds the published six-strategy registry with the actual layout implementations.
///
/// The caller still supplies format-specific evidence, request policy and IPC wiring.
pub fn default_dispatcher() -> Result<Dispatcher, Error> {
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    register_implemented(&mut builder)?;
    register_bar_candidate(&mut builder)?;
    register_trend_candidate(&mut builder)?;
    register_waterfall_candidate(&mut builder)?;
    register_histogram_candidate(&mut builder)?;
    register_sankey_candidate(&mut builder)?;
    finish_default(builder)
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
    let bar = typed_layout::bar_descriptor();
    if registry
        .resolve(&bar.format, &bar.strategy)
        .map_or(true, |entry| !entry.implementation.complete_bar())
    {
        return Err(Error::MissingRequired("viz.bar"));
    }
    for (format, id) in OTHER_REQUIRED {
        registry
            .resolve(
                format,
                &LayoutStrategyKey {
                    id: id.into(),
                    revision: 1,
                },
            )
            .map_err(|_| Error::MissingRequired(id))?;
    }
    Ok(Dispatcher::new(registry))
}
