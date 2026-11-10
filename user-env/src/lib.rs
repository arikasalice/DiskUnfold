//! Own settings, workspaces and localization.

use std::collections::BTreeMap;

use du_core_contracts::settings::SettingsSnapshot;
pub use du_core_contracts::settings::{
    SettingDescriptor as Descriptor, SettingValueKind as ValueKind,
};
use serde_json::Value;

pub mod i18n;
pub mod locale;
pub mod settings;
pub mod space;
pub mod space_format;

fn accepts(kind: &ValueKind, value: &Value) -> bool {
    match kind {
        ValueKind::Boolean => value.is_boolean(),
        ValueKind::Integer { min, max } => {
            value.as_i64().is_some_and(|v| (*min..=*max).contains(&v))
        }
        ValueKind::Choice(values) => values.contains(value),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RegistryError {
    DuplicateKey,
    InvalidDefault,
}

pub struct Registry {
    descriptors: BTreeMap<String, Descriptor>,
}

pub struct CatalogEntry<'a> {
    pub descriptor: &'a Descriptor,
    pub value: &'a Value,
    pub uses_default: bool,
}

impl Registry {
    pub fn new(descriptors: impl IntoIterator<Item = Descriptor>) -> Result<Self, RegistryError> {
        let mut result = BTreeMap::new();
        for descriptor in descriptors {
            if !accepts(&descriptor.kind, &descriptor.default) {
                return Err(RegistryError::InvalidDefault);
            }
            if result.insert(descriptor.key.clone(), descriptor).is_some() {
                return Err(RegistryError::DuplicateKey);
            }
        }
        Ok(Self {
            descriptors: result,
        })
    }

    pub fn catalog<'a>(&'a self, snapshot: &'a SettingsSnapshot) -> Vec<CatalogEntry<'a>> {
        self.descriptors
            .values()
            .map(|descriptor| {
                let custom = snapshot.values.get(&descriptor.key);
                CatalogEntry {
                    descriptor,
                    value: custom.unwrap_or(&descriptor.default),
                    uses_default: custom.is_none(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_rejects_duplicates_and_catalog_uses_the_given_snapshot() -> Result<(), RegistryError>
    {
        let descriptor = Descriptor {
            key: "locale.language".into(),
            owner: "F13".into(),
            default: json!("auto"),
            kind: ValueKind::Choice(vec![json!("auto"), json!("ko")]),
            editable: true,
        };
        assert!(Registry::new([descriptor.clone(), descriptor.clone()]).is_err());
        let registry = Registry::new([descriptor])?;
        let empty = SettingsSnapshot::default();
        let catalog = registry.catalog(&empty);
        assert!(catalog[0].uses_default);
        assert_eq!(catalog[0].value, &json!("auto"));
        let custom = SettingsSnapshot {
            revision: 1,
            schema_version: 1,
            values: BTreeMap::from([("locale.language".into(), json!("ko"))]),
        };
        let catalog = registry.catalog(&custom);
        assert!(!catalog[0].uses_default);
        assert_eq!(catalog[0].value, &json!("ko"));
        Ok(())
    }
}
