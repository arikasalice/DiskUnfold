//! Open keys shared by visualization strategy registration and its consumers.

pub mod geometry;
pub mod quantity;

/// Versioned key used to select an algorithm and identify its output.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LayoutStrategyKey {
    pub id: String,
    pub revision: u32,
}

/// A format and strategy revision provided by the same registry snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutDescriptor {
    pub format: String,
    pub strategy: LayoutStrategyKey,
}
