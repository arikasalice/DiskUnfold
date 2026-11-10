//! Internal view of the registered F06 layout strategies.
//!
//! This is not the complete `viz.catalog` response: measures, color and
//! denominator criteria, table revisions and policy must come from their owning
//! registries before the public use case can be assembled.
use du_analysis_index::visualization::{registry::SelectionError, typed_layout::Dispatcher};
use du_core_contracts::visualization::{LayoutDescriptor, LayoutStrategyKey};

/// Registry data available without inventing support for unregistered formats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    /// Unique formats in their first occurrence in registry iteration order.
    pub formats: Vec<String>,
    /// Every registered strategy and its associated open format key.
    pub strategies: Vec<LayoutDescriptor>,
}

/// Internal catalog or selection failure; transport mapping is owned by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    EmptyRegistry,
    UnknownFormat,
    UnknownStrategy,
}

/// Takes one immutable snapshot of the actual layout registry.
///
/// The registered order is preserved, rather than defining a new public sorting
/// convention. An empty registry is an error, not an empty successful catalog.
pub fn snapshot(dispatcher: &Dispatcher) -> Result<Catalog, Error> {
    let strategies: Vec<_> = dispatcher.catalog().cloned().collect();
    if strategies.is_empty() {
        return Err(Error::EmptyRegistry);
    }
    let mut formats = Vec::new();
    for entry in &strategies {
        if !formats.contains(&entry.format) {
            formats.push(entry.format.clone());
        }
    }
    Ok(Catalog {
        formats,
        strategies,
    })
}

/// Resolves a registered format and strategy without executing a layout.
///
/// An omitted strategy selects the highest registered revision for that format;
/// an explicit ID and revision must match exactly. Format names are open keys.
pub fn select(
    dispatcher: &Dispatcher,
    format: &str,
    strategy: Option<&LayoutStrategyKey>,
) -> Result<LayoutDescriptor, Error> {
    dispatcher
        .resolve(format, strategy)
        .cloned()
        .map_err(|error| match error {
            SelectionError::UnknownFormat => Error::UnknownFormat,
            SelectionError::UnknownStrategy => Error::UnknownStrategy,
        })
}
