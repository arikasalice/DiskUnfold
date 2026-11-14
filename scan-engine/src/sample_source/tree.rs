//! Immutable tree of one sample observation point.
use std::collections::HashMap;

use du_core_contracts::{
    Timestamp,
    ports::{PortError, PortErrorKind},
    scan_observation::{MetadataObservation, ObservedEntry, ObservedKind, SpaceIdentity},
};

use crate::sample_bundle::{SampleItem, SampleItemKind, SampleObservationState, ValidatedBundle};

use super::SampleSourceError;

/// Prefix no real adapter can produce: real volume identities never start with a NUL-terminated
/// ASCII marker followed by a bundle version.
pub const SAMPLE_VOLUME_PREFIX: &[u8] = b"du-sample\0";
/// Index of the root node.
pub(super) const ROOT: usize = 0;
/// Device number reported for every sample entry.
const SAMPLE_DEVICE: u64 = 1;

/// One directory or item of the tree. The root has no item.
pub(super) struct Node {
    pub(super) item: Option<SampleItem>,
    pub(super) entry: Option<ObservedEntry>,
    /// Child node indexes ordered by name bytes.
    pub(super) children: Vec<usize>,
    pub(super) file_id: Vec<u8>,
    /// Identities of the verified ancestors, excluding this node.
    pub(super) ancestors: Vec<Vec<u8>>,
}

/// All entries of one observation point plus the synthetic content they read.
pub(super) struct SampleTree {
    pub(super) volume_id: Vec<u8>,
    pub(super) observed_at: Timestamp,
    pub(super) nodes: Vec<Node>,
    pub(super) by_path: HashMap<Vec<String>, usize>,
    pub(super) fragments: Vec<String>,
}

impl SampleTree {
    pub(super) fn build(
        bundle: &ValidatedBundle,
        observation: usize,
    ) -> Result<Self, SampleSourceError> {
        let point = bundle
            .observations()
            .get(observation)
            .ok_or(SampleSourceError::ObservationMissing)?;
        let mut nodes = vec![Node {
            item: None,
            entry: None,
            children: Vec::new(),
            file_id: file_id(&[]),
            ancestors: Vec::new(),
        }];
        let mut by_path: HashMap<Vec<String>, usize> = HashMap::new();
        for item in &point.items {
            let parent_path = &item.path[..item.path.len() - 1];
            let parent = if parent_path.is_empty() {
                ROOT
            } else {
                *by_path
                    .get(parent_path)
                    .ok_or(SampleSourceError::Inconsistent)?
            };
            let mut ancestors = nodes[parent].ancestors.clone();
            ancestors.push(nodes[parent].file_id.clone());
            let index = nodes.len();
            nodes.push(Node {
                entry: Some(entry(item)),
                item: Some(item.clone()),
                children: Vec::new(),
                file_id: file_id(&item.path),
                ancestors,
            });
            nodes[parent].children.push(index);
            by_path.insert(item.path.clone(), index);
        }
        sort_children(&mut nodes);
        Ok(Self {
            volume_id: volume_id(&bundle.header().bundle_version),
            observed_at: point.observed_at.clone(),
            nodes,
            by_path,
            fragments: bundle.records().content_fragments.clone(),
        })
    }
}

fn sort_children(nodes: &mut [Node]) {
    let names: Vec<Vec<u8>> = nodes
        .iter()
        .map(|node| {
            node.entry
                .as_ref()
                .map(|entry| entry.name.clone())
                .unwrap_or_default()
        })
        .collect();
    for node in nodes.iter_mut() {
        node.children.sort_by(|a, b| names[*a].cmp(&names[*b]));
    }
}

/// Returns the volume identity bytes: the sample prefix followed by the bundle version.
pub(super) fn volume_id(bundle_version: &str) -> Vec<u8> {
    let mut id = SAMPLE_VOLUME_PREFIX.to_vec();
    id.extend_from_slice(bundle_version.as_bytes());
    id
}

fn inode(path: &[String]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    for component in path {
        hasher.update(component.as_bytes());
        hasher.update(&[0]);
    }
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest.as_bytes()[..8]);
    // Zero means "not observed" to consumers, so keep the identity nonzero.
    u64::from_le_bytes(bytes) | 1
}

fn file_id(path: &[String]) -> Vec<u8> {
    inode(path).to_le_bytes().to_vec()
}

fn port_error(kind: PortErrorKind, reason_key: &str) -> PortError {
    PortError {
        kind,
        reason_key: reason_key.to_owned(),
        os_code: None,
    }
}

/// Carries the item's observation state example in the matching observation record field.
fn entry(item: &SampleItem) -> ObservedEntry {
    let kind = match item.kind {
        SampleItemKind::Directory => ObservedKind::Directory,
        SampleItemKind::File => ObservedKind::File,
        SampleItemKind::Link => ObservedKind::Link,
    };
    let inode = inode(&item.path);
    let metadata = match item.state {
        Some(SampleObservationState::AccessDenied) => Err(port_error(
            PortErrorKind::PermissionDenied,
            "os.query_failed",
        )),
        Some(SampleObservationState::Vanished) => {
            Err(port_error(PortErrorKind::NotFound, "os.not_found"))
        }
        _ => Ok(MetadataObservation {
            kind: kind.clone(),
            device: SAMPLE_DEVICE,
            inode,
            size: if item.state == Some(SampleObservationState::SizeUnknown) {
                Err(port_error(PortErrorKind::Io, "os.query_failed"))
            } else {
                Ok(item.size)
            },
            modified_at: item.modified_at.clone(),
            native_modified_at: None,
            allocated: item.allocated,
            link_count: Some(1),
            space_identity: Some(SpaceIdentity {
                volume: Vec::new(),
                file: inode.to_le_bytes().to_vec(),
            }),
        }),
    };
    ObservedEntry {
        name: item
            .path
            .last()
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_default(),
        enumerated_inode: inode,
        enumerated_kind: kind,
        metadata,
    }
}
