//! Registered Sankey calculation over an injected pinned-attribution boundary.

use crate::visualization::{geometry::Weight, sankey};
use du_core_contracts::visualization::{LayoutDescriptor, LayoutStrategyKey};

/// Facts supplied by a trusted, fixed-result attribution provider.
///
/// The strategy validates numeric and classification consistency, but cannot
/// prove the provider's result-version identity or disjoint attribution. The
/// provider is an explicit trust boundary and must pin those facts.
pub struct SankeyInput<'a> {
    pub columns: sankey::Columns,
    pub scope: Weight,
    pub rows: &'a [sankey::Row],
    pub folder_budget: usize,
    pub category_order: &'a [u32],
}

/// Default F06 version-one Sankey strategy.
pub struct SankeyLayout;

impl super::Strategy for SankeyLayout {
    fn sankey(
        &self,
        input: SankeyInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<SankeyOutput, SankeyDispatchError> {
        if cancelled() {
            return Err(SankeyDispatchError::Cancelled);
        }
        let graph = match sankey::layout_cancellable(
            input.columns,
            input.scope,
            input.rows,
            input.folder_budget,
            input.category_order,
            cancelled,
        ) {
            sankey::Outcome::Graph(graph) => graph,
            sankey::Outcome::Cancelled => return Err(SankeyDispatchError::Cancelled),
            sankey::Outcome::Unavailable(reason) => {
                return Err(SankeyDispatchError::Unavailable(reason));
            }
            sankey::Outcome::Invalid(reason) => return Err(SankeyDispatchError::Invalid(reason)),
        };
        if cancelled() {
            return Err(SankeyDispatchError::Cancelled);
        }
        let relations = match graph.geometry_v1_cancellable(cancelled) {
            Ok(relations) => relations,
            Err(sankey::geometry_v1::Error::Cancelled) => {
                return Err(SankeyDispatchError::Cancelled);
            }
            Err(error) => {
                return Err(SankeyDispatchError::Geometry {
                    graph: Box::new(graph),
                    error,
                });
            }
        };
        if cancelled() {
            return Err(SankeyDispatchError::Cancelled);
        }
        Ok(SankeyOutput { graph, relations })
    }
}

/// Graph and checked relationship geometry from the same pinned facts.
#[derive(Debug)]
pub struct SankeyOutput {
    pub graph: sankey::Graph,
    pub relations: sankey::geometry_v1::Relations,
}

/// Selection or calculation failure, preserving the graph on geometry errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SankeyDispatchError {
    Selection(super::registry::SelectionError),
    Unsupported,
    Unavailable(sankey::Unavailable),
    Invalid(sankey::Invalid),
    Geometry {
        graph: Box<sankey::Graph>,
        error: sankey::geometry_v1::Error,
    },
    Cancelled,
}

/// A selected Sankey result and its registered strategy identity.
#[derive(Debug)]
pub struct SankeySelected {
    pub descriptor: LayoutDescriptor,
    pub output: SankeyOutput,
}

impl super::Dispatcher {
    /// Selects a registered strategy and calculates Sankey geometry.
    pub fn execute_sankey(
        &self,
        key: Option<&LayoutStrategyKey>,
        input: SankeyInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<SankeySelected, SankeyDispatchError> {
        let entry = match key {
            Some(key) => self.0.resolve("sankey", key),
            None => self.0.latest("sankey"),
        }
        .map_err(SankeyDispatchError::Selection)?;
        let output = entry.implementation.sankey(input, cancelled)?;
        Ok(SankeySelected {
            descriptor: entry.descriptor.clone(),
            output,
        })
    }
}

/// F06 Sankey descriptor at the published strategy revision.
pub fn descriptor() -> LayoutDescriptor {
    LayoutDescriptor {
        format: "sankey".into(),
        strategy: LayoutStrategyKey {
            id: "viz.sankey".into(),
            revision: 1,
        },
    }
}
