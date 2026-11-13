//! Registry of snapshot format versions that may be read (FR8.HD1.snapshot-files.DD1).

use std::collections::BTreeSet;

use du_core_contracts::snapshot_format::SnapshotFormatRegistry;

/// Format versions registered by the composition root, one registration per reader module.
///
/// The registry holds no file state and never writes. A new format version adds its reader
/// module to the snapshot store and one `register` call where the registry is assembled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormatRegistry {
    versions: BTreeSet<u16>,
}

impl FormatRegistry {
    /// Creates a registry in which no version is registered, so every file is unreadable.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `version`. Returns `false` when it was already registered.
    pub fn register(&mut self, version: u16) -> bool {
        self.versions.insert(version)
    }

    /// Returns the registered versions in ascending order.
    pub fn versions(&self) -> impl Iterator<Item = u16> + '_ {
        self.versions.iter().copied()
    }
}

impl SnapshotFormatRegistry for FormatRegistry {
    fn is_registered(&self, version: u16) -> bool {
        self.versions.contains(&version)
    }
}

impl FromIterator<u16> for FormatRegistry {
    fn from_iter<I: IntoIterator<Item = u16>>(versions: I) -> Self {
        Self {
            versions: versions.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_registered_versions_are_known() {
        let mut registry = FormatRegistry::new();
        assert!(!registry.is_registered(1));
        assert!(registry.register(1));
        assert!(!registry.register(1));
        assert!(registry.is_registered(1));
        assert!(!registry.is_registered(2));
        assert!(!registry.is_registered(0));
    }

    #[test]
    fn versions_are_listed_in_order_without_duplicates() {
        let registry: FormatRegistry = [3, 1, 3, 2].into_iter().collect();
        assert_eq!(registry.versions().collect::<Vec<_>>(), [1, 2, 3]);
    }
}
