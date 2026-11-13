//! Read-only settings search using the registered language resources.
use std::{collections::BTreeMap, ops::Range};

use du_core_contracts::settings::{SettingsSnapshot, SettingsStore};
use icu_casemap::CaseMapper;
use icu_normalizer::ComposingNormalizer;

use crate::{CatalogEntry, Registry, i18n};

use super::SettingsService;

/// The searched field. Language identifies the actual registered resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Field {
    Key,
    Name(&'static str),
    Description(&'static str),
}

/// An internal match in the NFC-then-folded field, not a source highlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub field: Field,
    /// UTF-8 byte range in `normalized`, never in the original resource.
    pub range: Range<usize>,
    pub normalized: String,
}

/// One catalog entry and its matching fields from a single settings snapshot.
pub struct Hit<'a> {
    pub entry: CatalogEntry<'a>,
    pub matches: Vec<Match>,
}

/// Search output tied to the same revision and values as the catalog.
pub struct Results<'a> {
    pub revision: u64,
    /// Korean was excluded at startup because its integrity check failed.
    pub korean_integrity_degraded: bool,
    pub hits: Vec<Hit<'a>>,
}

/// Search cannot silently omit resources or decide an unspecified empty query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    MissingLanguage,
    MissingMessage,
    EmptyQuery,
}

fn normalize(value: &str) -> String {
    let nfc = ComposingNormalizer::new_nfc();
    let normalized = nfc.normalize(value);
    CaseMapper::new().fold_string(&normalized).into_owned()
}

fn messages(bundle: &i18n::Bundle) -> BTreeMap<&str, &str> {
    bundle
        .messages
        .iter()
        .map(|(key, value)| (key.as_str(), value.text.as_str()))
        .collect()
}

impl Registry {
    /// Searches the exact catalog snapshot and both verified language bundles.
    ///
    /// The first normalized substring occurrence is reported for every matching
    /// field. Whitespace is searched literally; empty-query handling and public
    /// match-coordinate representation remain to be specified by the caller.
    /// Missing bundles or message keys reject the entire search. A Korean
    /// integrity failure recorded by startup permits English-only search and
    /// is reported in the result without publishing a second warning.
    ///
    /// # Errors
    /// Returns `EmptyQuery`, `MissingLanguage`, or `MissingMessage`.
    pub fn search<'a>(
        &'a self,
        snapshot: &'a SettingsSnapshot,
        resources: &i18n::Registry,
        query: &str,
    ) -> Result<Results<'a>, Error> {
        let query = normalize(query);
        if query.is_empty() {
            return Err(Error::EmptyQuery);
        }
        let en = messages(resources.bundle("en").ok_or(Error::MissingLanguage)?);
        let ko = match resources.bundle("ko") {
            Some(bundle) => Some(messages(bundle)),
            None if resources
                .configured_languages()
                .any(|language| language == "ko")
                && resources
                    .integrity_warnings()
                    .iter()
                    .any(|key| key == "i18n.bundle_integrity_failed") =>
            {
                None
            }
            None => return Err(Error::MissingLanguage),
        };
        let mut hits = Vec::new();
        for entry in self.catalog(snapshot) {
            let presentation = &entry.descriptor.presentation;
            let mut fields = vec![
                (Field::Key, entry.descriptor.key.as_str()),
                (
                    Field::Name("en"),
                    *en.get(presentation.name_key.as_str())
                        .ok_or(Error::MissingMessage)?,
                ),
                (
                    Field::Description("en"),
                    *en.get(presentation.description_key.as_str())
                        .ok_or(Error::MissingMessage)?,
                ),
            ];
            if let Some(ko) = &ko {
                fields.push((
                    Field::Name("ko"),
                    *ko.get(presentation.name_key.as_str())
                        .ok_or(Error::MissingMessage)?,
                ));
                fields.push((
                    Field::Description("ko"),
                    *ko.get(presentation.description_key.as_str())
                        .ok_or(Error::MissingMessage)?,
                ));
            }
            let matches = fields
                .into_iter()
                .filter_map(|(field, value)| {
                    let normalized = normalize(value);
                    let start = normalized.find(&query)?;
                    Some(Match {
                        field,
                        range: start..start + query.len(),
                        normalized,
                    })
                })
                .collect::<Vec<_>>();
            if !matches.is_empty() {
                hits.push(Hit { entry, matches });
            }
        }
        Ok(Results {
            revision: snapshot.revision,
            korean_integrity_degraded: ko.is_none(),
            hits,
        })
    }
}

impl<S: SettingsStore> SettingsService<S> {
    /// Searches the same immutable snapshot returned by `catalog`.
    ///
    /// # Errors
    /// Propagates resource and empty-query search errors.
    pub fn search<'a>(
        &'a self,
        resources: &i18n::Registry,
        query: &str,
    ) -> Result<Results<'a>, Error> {
        self.registry.search(&self.snapshot, resources, query)
    }
}

#[cfg(test)]
mod tests;
