//! Sample source: one observation point of a validated bundle behind the scan ports.
//!
//! The source answers `ScopeRootPort`, `AnchoredDirectory` and `ContentReadPort` from memory.
//! It never calls `platform-os` and never touches the file system or the network.
mod capability;
mod directory;
mod tree;

use std::sync::Arc;

use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    ports::{ContentReadPort, PortError, PortErrorKind},
    scan_observation::{OpenedScopeRoot, ScopeRootPort},
};

pub use capability::{SampleCapability, sample_capability};
pub use tree::SAMPLE_VOLUME_PREFIX;

use crate::sample_bundle::{SampleItemKind, ValidatedBundle};
use directory::SampleDirectory;
use tree::{ROOT, SampleTree};

/// Absolute path under which the sample root can be opened.
pub const SAMPLE_ROOT_PATH: &[u8] = b"/sample";

/// The requested observation point cannot be served.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleSourceError {
    /// The bundle has no observation point with that index.
    ObservationMissing,
    /// The decoded records reference a missing parent (a validated bundle never does).
    Inconsistent,
}

/// Serves one immutable observation point of a validated bundle.
#[derive(Clone)]
pub struct SampleScope {
    tree: Arc<SampleTree>,
}

impl std::fmt::Debug for SampleScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SampleScope")
            .field("items", &self.item_count())
            .finish()
    }
}

impl SampleScope {
    /// Builds the source for the observation point at `observation` (bundle order).
    pub fn new(bundle: &ValidatedBundle, observation: usize) -> Result<Self, SampleSourceError> {
        Ok(Self {
            tree: Arc::new(SampleTree::build(bundle, observation)?),
        })
    }

    /// Returns the number of items below the root.
    pub fn item_count(&self) -> usize {
        self.tree.nodes.len() - 1
    }

    /// Returns the volume identity bytes, which start with [`SAMPLE_VOLUME_PREFIX`].
    pub fn volume_id(&self) -> &[u8] {
        &self.tree.volume_id
    }

    /// Returns the time the observation point represents.
    pub fn observed_at(&self) -> &du_core_contracts::Timestamp {
        &self.tree.observed_at
    }
}

fn error(kind: PortErrorKind, reason_key: &str) -> PortError {
    PortError {
        kind,
        reason_key: reason_key.to_owned(),
        os_code: None,
    }
}

impl ScopeRootPort for SampleScope {
    fn open_root(&self, absolute_path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        if absolute_path != SAMPLE_ROOT_PATH {
            return Err(error(PortErrorKind::NotFound, "os.not_found"));
        }
        Ok(OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(SAMPLE_ROOT_PATH),
                display: String::from_utf8_lossy(SAMPLE_ROOT_PATH).into_owned(),
                lossy: false,
            },
            volume_id: self.tree.volume_id.clone(),
            file_id: self.tree.nodes[ROOT].file_id.clone(),
            directory: Box::new(SampleDirectory::new(Arc::clone(&self.tree), ROOT)),
        })
    }
}

impl ContentReadPort for SampleScope {
    /// Returns the item's synthetic fragment repeated up to its logical size and `limit`.
    /// Items that share a fragment and size read identical bytes.
    fn read(&self, path: &[u8], limit: usize) -> Result<Vec<u8>, PortError> {
        let not_found = || error(PortErrorKind::NotFound, "os.not_found");
        let relative = path
            .strip_prefix(SAMPLE_ROOT_PATH)
            .and_then(|rest| rest.strip_prefix(b"/"))
            .ok_or_else(not_found)?;
        let components: Vec<String> = relative
            .split(|byte| *byte == b'/')
            .map(|part| String::from_utf8(part.to_vec()).map_err(|_| not_found()))
            .collect::<Result<_, _>>()?;
        let node = self.tree.by_path.get(&components).ok_or_else(not_found)?;
        let item = self.tree.nodes[*node].item.as_ref().ok_or_else(not_found)?;
        let fragment = match (item.kind, item.content) {
            (SampleItemKind::File, Some(index)) => self.tree.fragments.get(index),
            _ => None,
        }
        .ok_or_else(|| error(PortErrorKind::Unsupported, "os.unsupported"))?;
        let wanted = usize::try_from(item.size).unwrap_or(usize::MAX).min(limit);
        Ok(fragment.bytes().cycle().take(wanted).collect())
    }
}
