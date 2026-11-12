//! Catalog of the registered F06 layout strategies and their policy tables.
//!
//! Composition supplies the measures, criteria and revisions from their owners.
//! Catalog assembly never executes a layout or probes a result source.
use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};
use du_analysis_index::visualization::{registry::SelectionError, typed_layout::Dispatcher};
use du_core_contracts::{
    browse_page_ipc::QueryStrategy,
    visualization::{
        LayoutDescriptor, LayoutStrategyKey,
        catalog::{VizCatalog, VizCatalogRequest},
    },
};
use std::collections::HashSet;

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
    InvalidPolicy,
}

/// Available criteria and immutable revisions from the owning policy tables.
pub struct Policy {
    pub measures: Vec<String>,
    pub color_criteria: Vec<String>,
    pub denominator_criteria: Vec<String>,
    pub interpretation_table_revision: u32,
    pub required_data_table_revision: u32,
    pub legend_rule_revision: u32,
}

/// Registers the GUI-only catalog from one validated registry and policy snapshot.
pub fn register(
    builder: &mut EngineBuilder,
    dispatcher: &Dispatcher,
    policy: Policy,
) -> Result<(), Error> {
    let catalog = snapshot(dispatcher)?;
    if ![
        &policy.measures,
        &policy.color_criteria,
        &policy.denominator_criteria,
    ]
    .into_iter()
    .all(|values| valid_keys(values))
        || [
            policy.interpretation_table_revision,
            policy.required_data_table_revision,
            policy.legend_rule_revision,
        ]
        .contains(&0)
    {
        return Err(Error::InvalidPolicy);
    }
    builder.register(ReadCatalog(VizCatalog {
        formats: catalog.formats,
        strategies: catalog
            .strategies
            .into_iter()
            .map(|entry| QueryStrategy {
                id: entry.strategy.id,
                revision: entry.strategy.revision,
            })
            .collect(),
        measures: policy.measures,
        color_criteria: policy.color_criteria,
        denominator_criteria: policy.denominator_criteria,
        interpretation_table_revision: policy.interpretation_table_revision,
        required_data_table_revision: policy.required_data_table_revision,
        legend_rule_revision: policy.legend_rule_revision,
    }));
    Ok(())
}

fn valid_keys(values: &[String]) -> bool {
    let mut seen = HashSet::new();
    !values.is_empty()
        && values
            .iter()
            .all(|key| !key.trim().is_empty() && seen.insert(key))
}

struct ReadCatalog(VizCatalog);

impl UseCase for ReadCatalog {
    const NAME: &'static str = "viz.catalog";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizCatalogRequest;
    type Response = VizCatalog;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        Ok(self.0.clone())
    }
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
