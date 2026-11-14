//! Sample bundle format version 1: byte layout, reader and registry.
//!
//! # Byte layout
//!
//! All integers are little endian. The body is, in order:
//!
//! | Offset | Size | Content |
//! | --- | --- | --- |
//! | 0 | 8 | magic bytes `DUSAMPLE` |
//! | 8 | 2 | format version, `u16` (this module reads 1) |
//! | 10 | 4 | header length `H`, `u32` |
//! | 14 | `H` | header JSON, UTF-8 |
//! | 14 + `H` | rest | payload JSON, UTF-8 |
//!
//! The header JSON has exactly these members: `bundleVersion` (string), `digestAlgorithm`
//! (`"blake3"`), `digest` (64 lowercase hexadecimal characters), `declaredItemCount` (number
//! of items across all observations) and `scenarioIds` (array of strings). The digest is the
//! BLAKE3 hash of the complete payload bytes, so `payload_range` is `14 + H..body.len()`.
//!
//! The payload JSON has `scenarios`, `observations`, `contentFragments`,
//! `exampleCapabilities`, `protectionRules` and `cleanupCandidates`. See
//! `assets/sample/bundle.v1.json` for a complete instance. Unknown members are rejected.
use std::collections::HashSet;
use std::sync::Arc;

use du_core_contracts::{Timestamp, volume::CapabilityState};

use super::raw::{
    RawCandidate, RawCapabilities, RawCapability, RawHeader, RawItem, RawKind, RawObservation,
    RawOperation, RawPayload, RawRule, RawState,
};
use super::{
    BundleDecodeLimits, BundleFormatReader, BundleFormatReaderRegistry, BundleHeader,
    CandidateOperation, CleanupCandidate, DecodedBundleRecords, DecodedObservation,
    ExampleCapabilities, FormatReaderError, ProtectionRule, RecordScenarioReference, SampleItem,
    SampleItemKind, SampleObservationState, SampleScenario,
};

/// Magic bytes that open every sample bundle.
pub const MAGIC: &[u8; 8] = b"DUSAMPLE";
/// The format version this module reads and writes.
pub const FORMAT_VERSION: u16 = 1;
/// Length of the fixed preamble: magic, format version and header length.
pub const PREAMBLE_LEN: usize = 14;
const MAX_HEADER_LEN: usize = 64 * 1024;
const MAX_COMPONENT_BYTES: usize = 255;

/// Reader for format version 1.
#[derive(Clone, Copy, Debug, Default)]
pub struct FormatV1Reader;

impl BundleFormatReader for FormatV1Reader {
    fn read_header(&self, body: &Arc<[u8]>) -> Result<BundleHeader, FormatReaderError> {
        let start = preamble(body).ok_or(FormatReaderError)?.1;
        let header_bytes = body.get(PREAMBLE_LEN..start).ok_or(FormatReaderError)?;
        let raw: RawHeader = serde_json::from_slice(header_bytes).map_err(|_| FormatReaderError)?;
        Ok(BundleHeader {
            bundle_version: raw.bundle_version,
            digest_algorithm: raw.digest_algorithm,
            digest: parse_digest(&raw.digest).ok_or(FormatReaderError)?,
            payload_range: start..body.len(),
            declared_item_count: raw.declared_item_count,
            scenario_ids: raw.scenario_ids,
        })
    }

    fn decode_records(
        &self,
        payload: &[u8],
        header: &BundleHeader,
        limits: BundleDecodeLimits,
    ) -> Result<DecodedBundleRecords, FormatReaderError> {
        let raw = parse_payload(payload)?;
        let ids: Vec<&str> = raw.scenarios.iter().map(|s| s.id.as_str()).collect();
        if ids
            != header
                .scenario_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        {
            return Err(FormatReaderError);
        }
        // The validator caps the body at 8 MiB before this call, which bounds the transient
        // parse above; the caps are applied before any record is converted and retained.
        let total_items: usize = raw.observations.iter().map(|o| o.items.len()).sum();
        if raw.observations.len() > limits.max_observations || total_items > limits.max_items {
            return Err(FormatReaderError);
        }
        convert(raw)
    }
}

/// Maps a format version to its reader. An unregistered version has no reader.
#[derive(Default)]
pub struct SampleFormatRegistry {
    readers: Vec<(u16, Box<dyn BundleFormatReader + Send + Sync>)>,
}

impl SampleFormatRegistry {
    /// Creates a registry that knows no format.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates the registry of formats this build can read: version 1.
    pub fn standard() -> Self {
        Self::new().register(FORMAT_VERSION, Box::new(FormatV1Reader))
    }

    /// Registers the reader for one format version.
    #[must_use]
    pub fn register(
        mut self,
        version: u16,
        reader: Box<dyn BundleFormatReader + Send + Sync>,
    ) -> Self {
        self.readers.push((version, reader));
        self
    }
}

impl BundleFormatReaderRegistry for SampleFormatRegistry {
    fn reader_for<'a>(&'a self, body: &Arc<[u8]>) -> Option<&'a dyn BundleFormatReader> {
        let version = body
            .strip_prefix(MAGIC.as_slice())
            .and_then(|rest| rest.get(..2))
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))?;
        self.readers
            .iter()
            .find(|(registered, _)| *registered == version)
            .map(|(_, reader)| reader.as_ref() as &dyn BundleFormatReader)
    }
}

/// Builds a format version 1 bundle from payload JSON bytes.
///
/// The header's item count and scenario ids are derived from the payload and its digest is the
/// BLAKE3 hash of the payload bytes. The payload must itself decode as a valid version 1 payload.
pub fn build_bundle(bundle_version: &str, payload: &[u8]) -> Result<Vec<u8>, FormatReaderError> {
    let raw = parse_payload(payload)?;
    let header = RawHeader {
        bundle_version: bundle_version.to_owned(),
        digest_algorithm: "blake3".to_owned(),
        digest: blake3::hash(payload).to_hex().to_string(),
        declared_item_count: raw.observations.iter().map(|o| o.items.len() as u64).sum(),
        scenario_ids: raw.scenarios.iter().map(|s| s.id.clone()).collect(),
    };
    convert(raw)?;
    let header_bytes = serde_json::to_vec(&header).map_err(|_| FormatReaderError)?;
    let header_len = u32::try_from(header_bytes.len()).map_err(|_| FormatReaderError)?;
    let mut out = Vec::with_capacity(PREAMBLE_LEN + header_bytes.len() + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&header_len.to_le_bytes());
    out.extend_from_slice(&header_bytes);
    out.extend_from_slice(payload);
    Ok(out)
}

/// Returns the format version and the payload start offset when the preamble is well formed.
fn preamble(body: &[u8]) -> Option<(u16, usize)> {
    let fixed = body.get(..PREAMBLE_LEN)?;
    if &fixed[..8] != MAGIC {
        return None;
    }
    let version = u16::from_le_bytes([fixed[8], fixed[9]]);
    let header_len = usize::try_from(u32::from_le_bytes([
        fixed[10], fixed[11], fixed[12], fixed[13],
    ]))
    .ok()?;
    let end = PREAMBLE_LEN.checked_add(header_len)?;
    (version == FORMAT_VERSION && header_len <= MAX_HEADER_LEN && end <= body.len())
        .then_some((version, end))
}

fn parse_digest(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut digest = [0u8; 32];
    for (slot, pair) in digest.iter_mut().zip(text.as_bytes().chunks(2)) {
        *slot = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(digest)
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn parse_payload(payload: &[u8]) -> Result<RawPayload, FormatReaderError> {
    serde_json::from_slice(payload).map_err(|_| FormatReaderError)
}

fn convert(raw: RawPayload) -> Result<DecodedBundleRecords, FormatReaderError> {
    let fragment_count = raw.content_fragments.len();
    if raw.content_fragments.iter().any(String::is_empty) {
        return Err(FormatReaderError);
    }
    let mut observations = Vec::with_capacity(raw.observations.len());
    let mut references = Vec::new();
    for (position, observation) in raw.observations.into_iter().enumerate() {
        let decoded = convert_observation(position, observation, fragment_count)?;
        references.extend(decoded.items.iter().map(|item| RecordScenarioReference {
            scenario_id: item.scenario.clone(),
        }));
        observations.push(decoded);
    }
    if observations
        .windows(2)
        .any(|pair| pair[1].observed_at.as_str() <= pair[0].observed_at.as_str())
    {
        return Err(FormatReaderError);
    }
    let last = observations.last().ok_or(FormatReaderError)?;
    let known: HashSet<&[String]> = last.items.iter().map(|i| i.path.as_slice()).collect();
    let protection_rules = convert_rules(raw.protection_rules, &known)?;
    let cleanup_candidates = convert_candidates(raw.cleanup_candidates, &known)?;
    Ok(DecodedBundleRecords {
        scenario_references: references,
        observations,
        scenarios: raw
            .scenarios
            .into_iter()
            .map(|s| SampleScenario {
                id: s.id,
                summary: s.summary,
            })
            .collect(),
        content_fragments: raw.content_fragments,
        example_capabilities: convert_capabilities(&raw.example_capabilities),
        protection_rules,
        cleanup_candidates,
    })
}

fn convert_observation(
    position: usize,
    raw: RawObservation,
    fragment_count: usize,
) -> Result<DecodedObservation, FormatReaderError> {
    if usize::try_from(raw.index).ok() != Some(position) {
        return Err(FormatReaderError);
    }
    let observed_at = Timestamp::from_unix_millis(raw.observed_at).ok_or(FormatReaderError)?;
    let mut items: Vec<SampleItem> = Vec::with_capacity(raw.items.len());
    // Directories that may still receive children, keyed by path.
    let mut open_dirs: HashSet<Vec<String>> = HashSet::new();
    let mut seen: HashSet<Vec<String>> = HashSet::new();
    for raw_item in raw.items {
        let item = convert_item(raw_item, fragment_count)?;
        check_placement(&item, &open_dirs, &mut seen)?;
        if item.kind == SampleItemKind::Directory && item.state.is_none() {
            open_dirs.insert(item.path.clone());
        }
        items.push(item);
    }
    Ok(DecodedObservation {
        index: raw.index,
        observed_at,
        items,
    })
}

fn check_placement(
    item: &SampleItem,
    open_dirs: &HashSet<Vec<String>>,
    seen: &mut HashSet<Vec<String>>,
) -> Result<(), FormatReaderError> {
    if !seen.insert(item.path.clone()) {
        return Err(FormatReaderError);
    }
    let parent = &item.path[..item.path.len() - 1];
    if !parent.is_empty() && !open_dirs.contains(parent) {
        return Err(FormatReaderError);
    }
    Ok(())
}

fn convert_item(raw: RawItem, fragment_count: usize) -> Result<SampleItem, FormatReaderError> {
    if raw.path.is_empty() || !raw.path.iter().all(|c| valid_component(c)) {
        return Err(FormatReaderError);
    }
    let kind = match raw.kind {
        RawKind::Dir => SampleItemKind::Directory,
        RawKind::File => SampleItemKind::File,
        RawKind::Link => SampleItemKind::Link,
    };
    let state = raw.state.map(|state| match state {
        RawState::AccessDenied => SampleObservationState::AccessDenied,
        RawState::Vanished => SampleObservationState::Vanished,
        RawState::LinkUntraversed => SampleObservationState::LinkUntraversed,
        RawState::SizeUnknown => SampleObservationState::SizeUnknown,
    });
    let modified_at = match raw.modified_at {
        Some(millis) => Some(Timestamp::from_unix_millis(millis).ok_or(FormatReaderError)?),
        None => None,
    };
    check_fields(kind, state, raw.size, raw.content, fragment_count)?;
    if raw.scenario.is_empty() {
        return Err(FormatReaderError);
    }
    Ok(SampleItem {
        path: raw.path,
        kind,
        size: raw.size,
        allocated: raw.allocated,
        modified_at,
        state,
        content: raw.content,
        scenario: raw.scenario,
    })
}

/// Rejects combinations of kind, state, size and content that no real observation produces.
fn check_fields(
    kind: SampleItemKind,
    state: Option<SampleObservationState>,
    size: u64,
    content: Option<usize>,
    fragment_count: usize,
) -> Result<(), FormatReaderError> {
    use SampleItemKind::{Directory, File, Link};
    use SampleObservationState::{AccessDenied, LinkUntraversed, SizeUnknown, Vanished};
    let state_fits = matches!(
        (kind, state),
        (_, None)
            | (Directory | File, Some(AccessDenied))
            | (File, Some(Vanished | SizeUnknown))
            | (Link, Some(LinkUntraversed))
    );
    let size_fits = kind == File && state != Some(SizeUnknown) || size == 0;
    let content_fits = match content {
        None => true,
        Some(index) => kind == File && state.is_none() && index < fragment_count,
    };
    if state_fits && size_fits && content_fits {
        Ok(())
    } else {
        Err(FormatReaderError)
    }
}

fn valid_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_COMPONENT_BYTES
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
}

fn convert_capabilities(raw: &RawCapabilities) -> ExampleCapabilities {
    let state = |value: RawCapability| match value {
        RawCapability::Supported => CapabilityState::Supported,
        RawCapability::Partial => CapabilityState::Partial,
        RawCapability::Unsupported => CapabilityState::Unsupported,
    };
    ExampleCapabilities {
        allocated_size: state(raw.allocated_size),
        link_detection: state(raw.link_detection),
        placeholder_detection: state(raw.placeholder_detection),
        extra_attributes: state(raw.extra_attributes),
    }
}

fn convert_rules(
    rules: Vec<RawRule>,
    known: &HashSet<&[String]>,
) -> Result<Vec<ProtectionRule>, FormatReaderError> {
    rules
        .into_iter()
        .map(|rule| {
            if rule.id.is_empty()
                || rule.reason_key.is_empty()
                || !known.contains(rule.path.as_slice())
            {
                return Err(FormatReaderError);
            }
            Ok(ProtectionRule {
                id: rule.id,
                path: rule.path,
                reason_key: rule.reason_key,
            })
        })
        .collect()
}

fn convert_candidates(
    candidates: Vec<RawCandidate>,
    known: &HashSet<&[String]>,
) -> Result<Vec<CleanupCandidate>, FormatReaderError> {
    candidates
        .into_iter()
        .map(|candidate| {
            if !known.contains(candidate.path.as_slice()) {
                return Err(FormatReaderError);
            }
            Ok(CleanupCandidate {
                operation: match candidate.operation {
                    RawOperation::Trash => CandidateOperation::Trash,
                    RawOperation::Move => CandidateOperation::Move,
                    RawOperation::Delete => CandidateOperation::Delete,
                },
                path: candidate.path,
            })
        })
        .collect()
}
