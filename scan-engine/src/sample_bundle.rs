//! Validation boundary for an immutable, embedded sample bundle.
//!
//! A registered format reader owns decoding (format version 1 is in [`format_v1`] and its byte
//! layout is documented there), while this module owns the ordered integrity and reference
//! checks: presence, format reader, digest, size and item limits, then record decoding. The first
//! failure stops the sequence and no partial bundle is ever exposed.

use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

pub mod embedded;
pub mod format_v1;
pub mod model;
mod raw;

pub use model::{
    CandidateOperation, CleanupCandidate, DecodedObservation, ExampleCapabilities, ProtectionRule,
    SampleItem, SampleItemKind, SampleObservationState, SampleScenario,
};

/// Largest accepted bundle body in bytes.
pub const MAX_BUNDLE_SIZE_BYTES: usize = 8 * 1024 * 1024;
/// Largest accepted item count across all observation points.
pub const MAX_BUNDLE_ITEMS: usize = 20_000;
/// Largest accepted number of observation points.
pub const MAX_OBSERVATIONS: usize = 2;

const MIN_OBSERVATIONS: usize = 1;
const BLAKE3_ALGORITHM: &str = "blake3";

/// Neutral validation outcome exposed to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundleFailure {
    Missing,
    UnsupportedFormat,
    Corrupt,
}

/// Reads the embedded asset once and returns the immutable byte body.
pub trait BundleSourcePort {
    fn read_bundle(&self) -> Option<Arc<[u8]>>;
}

/// Resolves a registered reader from the format marker in the same immutable body.
pub trait BundleFormatReaderRegistry {
    fn reader_for<'a>(&'a self, body: &Arc<[u8]>) -> Option<&'a dyn BundleFormatReader>;
}

/// Decodes one published format without exposing a partially parsed bundle.
pub trait BundleFormatReader {
    /// Parses only the selected format's header and declared metadata.
    fn read_header(&self, body: &Arc<[u8]>) -> Result<BundleHeader, FormatReaderError>;

    /// Parses and validates every record and observation in the verified payload slice.
    /// Implementations must honor both limits while decoding and must not return a prefix.
    fn decode_records(
        &self,
        payload: &[u8],
        header: &BundleHeader,
        limits: BundleDecodeLimits,
    ) -> Result<DecodedBundleRecords, FormatReaderError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatReaderError;

/// Format-neutral metadata needed by the validator and the sample source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleHeader {
    pub bundle_version: String,
    pub digest_algorithm: String,
    pub digest: [u8; 32],
    /// Format-specific body interval, excluding the header digest stored in the asset.
    pub payload_range: Range<usize>,
    pub declared_item_count: u64,
    pub scenario_ids: Vec<String>,
}

/// The scenario reference decoded from one complete bundle record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordScenarioReference {
    pub scenario_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Hard caps that a format reader must apply before retaining decoded entries.
pub struct BundleDecodeLimits {
    pub max_items: usize,
    pub max_observations: usize,
}

/// Complete decode result used to validate counts and scenario references and then retained
/// unchanged by the validated bundle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecodedBundleRecords {
    /// One reference per decoded item, across all observations.
    pub scenario_references: Vec<RecordScenarioReference>,
    /// Observation points in bundle order.
    pub observations: Vec<DecodedObservation>,
    /// Scenarios the payload declares.
    pub scenarios: Vec<SampleScenario>,
    /// Synthetic content fragments referenced by item content numbers.
    pub content_fragments: Vec<String>,
    /// Capability states the bundle records as examples.
    pub example_capabilities: ExampleCapabilities,
    /// Example protection rules for the sample cleanup plan.
    pub protection_rules: Vec<ProtectionRule>,
    /// Example cleanup candidates per operation kind.
    pub cleanup_candidates: Vec<CleanupCandidate>,
}

/// A view is constructed only after digest, limits, records and references all pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedBundle {
    body: Arc<[u8]>,
    payload_range: Range<usize>,
    header: BundleHeader,
    records: Arc<DecodedBundleRecords>,
}

impl ValidatedBundle {
    /// Returns the digest-verified payload bytes.
    pub fn record_bytes(&self) -> &[u8] {
        &self.body[self.payload_range.clone()]
    }

    /// Returns the verified header.
    pub fn header(&self) -> &BundleHeader {
        &self.header
    }

    /// Returns one scenario reference per decoded item.
    pub fn record_references(&self) -> &[RecordScenarioReference] {
        &self.records.scenario_references
    }

    /// Returns the number of observation points.
    pub fn observation_count(&self) -> usize {
        self.records.observations.len()
    }

    /// Returns the immutable decoded records.
    pub fn records(&self) -> &DecodedBundleRecords {
        &self.records
    }

    /// Returns the observation points in bundle order.
    pub fn observations(&self) -> &[DecodedObservation] {
        &self.records.observations
    }
}

/// Runs the ordered validation and returns the bundle only when every step passed.
pub fn validate_bundle(
    source: &dyn BundleSourcePort,
    readers: &dyn BundleFormatReaderRegistry,
) -> Result<ValidatedBundle, BundleFailure> {
    validate_with_digest(source, readers, |body| *blake3::hash(body).as_bytes())
}

fn validate_with_digest(
    source: &dyn BundleSourcePort,
    readers: &dyn BundleFormatReaderRegistry,
    digest_body: impl FnOnce(&[u8]) -> [u8; 32],
) -> Result<ValidatedBundle, BundleFailure> {
    let body = source.read_bundle().ok_or(BundleFailure::Missing)?;
    let reader = readers
        .reader_for(&body)
        .ok_or(BundleFailure::UnsupportedFormat)?;
    let header = reader
        .read_header(&body)
        .map_err(|_| BundleFailure::Corrupt)?;

    let payload = verify_digest(&body, &header, digest_body)?;
    let expected_items = check_limits(&body, &header)?;
    let decoded = reader
        .decode_records(
            payload,
            &header,
            BundleDecodeLimits {
                max_items: MAX_BUNDLE_ITEMS,
                max_observations: MAX_OBSERVATIONS,
            },
        )
        .map_err(|_| BundleFailure::Corrupt)?;
    validate_references(&header, &decoded.scenario_references, expected_items)?;
    validate_observations(&decoded.observations)?;

    Ok(ValidatedBundle {
        body,
        payload_range: header.payload_range.clone(),
        header,
        records: Arc::new(decoded),
    })
}

fn verify_digest<'a>(
    resource: &'a [u8],
    header: &BundleHeader,
    digest_body: impl FnOnce(&[u8]) -> [u8; 32],
) -> Result<&'a [u8], BundleFailure> {
    let payload = resource
        .get(header.payload_range.clone())
        .ok_or(BundleFailure::Corrupt)?;
    if header.digest_algorithm != BLAKE3_ALGORITHM || digest_body(payload) != header.digest {
        return Err(BundleFailure::Corrupt);
    }
    Ok(payload)
}

fn check_limits(body: &[u8], header: &BundleHeader) -> Result<usize, BundleFailure> {
    let item_count =
        usize::try_from(header.declared_item_count).map_err(|_| BundleFailure::Corrupt)?;
    if body.len() > MAX_BUNDLE_SIZE_BYTES || item_count > MAX_BUNDLE_ITEMS {
        return Err(BundleFailure::Corrupt);
    }
    Ok(item_count)
}

fn validate_observations(observations: &[DecodedObservation]) -> Result<(), BundleFailure> {
    if !(MIN_OBSERVATIONS..=MAX_OBSERVATIONS).contains(&observations.len()) {
        return Err(BundleFailure::Corrupt);
    }
    Ok(())
}

fn validate_references(
    header: &BundleHeader,
    record_references: &[RecordScenarioReference],
    expected_items: usize,
) -> Result<(), BundleFailure> {
    if record_references.len() != expected_items {
        return Err(BundleFailure::Corrupt);
    }

    let mut scenario_ids = HashSet::with_capacity(header.scenario_ids.len());
    for scenario_id in &header.scenario_ids {
        scenario_ids.insert(scenario_id.as_str());
    }
    if record_references
        .iter()
        .any(|reference| !scenario_ids.contains(reference.scenario_id.as_str()))
    {
        return Err(BundleFailure::Corrupt);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
