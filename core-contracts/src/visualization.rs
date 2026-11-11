//! Open keys shared by visualization strategy registration and its consumers.

pub mod geometry;
pub mod locate;
pub mod model;
pub mod quantity;
pub mod render;
pub mod request;
pub mod response;
pub mod rows;

use serde::{Deserialize, Deserializer};

fn optional_present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

fn required_nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}

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
