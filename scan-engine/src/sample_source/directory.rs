//! Directory handle over a sample tree.
use std::sync::Arc;

use du_core_contracts::{
    Timestamp,
    ports::{PortError, PortErrorKind},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, FilterDecision, FilteredBatch, ObservedEntry,
        ObservedKind,
    },
};

use super::tree::{ROOT, SampleTree};
use crate::sample_bundle::SampleObservationState;

/// Entries returned per batch; an empty batch ends the directory.
const BATCH: usize = 64;

/// A directory of the sample tree with its own enumeration cursor.
pub(super) struct SampleDirectory {
    tree: Arc<SampleTree>,
    node: usize,
    cursor: usize,
}

impl SampleDirectory {
    pub(super) fn new(tree: Arc<SampleTree>, node: usize) -> Self {
        Self {
            tree,
            node,
            cursor: 0,
        }
    }

    fn child_entry(&self, position: usize) -> Option<&ObservedEntry> {
        let child = *self.tree.nodes[self.node].children.get(position)?;
        self.tree.nodes[child].entry.as_ref()
    }
}

fn error(kind: PortErrorKind, reason_key: &str) -> PortError {
    PortError {
        kind,
        reason_key: reason_key.to_owned(),
        os_code: None,
    }
}

impl DirectoryObservations for SampleDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let mut batch = Vec::new();
        while batch.len() < BATCH {
            let Some(entry) = self.child_entry(self.cursor) else {
                break;
            };
            batch.push(entry.clone());
            self.cursor += 1;
        }
        Ok(batch)
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        let mut batch = Vec::new();
        while batch.len() < BATCH {
            let Some(entry) = self.child_entry(self.cursor) else {
                break;
            };
            match include(&entry.name) {
                FilterDecision::Admit => batch.push(entry.clone()),
                FilterDecision::Exclude => {}
                FilterDecision::Undecidable => return Ok(FilteredBatch::Undecidable),
            }
            self.cursor += 1;
        }
        Ok(if batch.is_empty() {
            FilteredBatch::End
        } else {
            FilteredBatch::Entries(batch)
        })
    }
}

impl AnchoredDirectory for SampleDirectory {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let nodes = &self.tree.nodes;
        let child = nodes[self.node]
            .children
            .iter()
            .copied()
            .find(|&index| {
                nodes[index]
                    .entry
                    .as_ref()
                    .is_some_and(|known| known.name == entry.name)
            })
            .ok_or_else(|| error(PortErrorKind::NotFound, "os.not_found"))?;
        let known = nodes[child].entry.as_ref().map(|e| e.enumerated_inode);
        if known != Some(entry.enumerated_inode) {
            return Err(error(PortErrorKind::IdentityChanged, "os.identity_changed"));
        }
        let state = nodes[child].item.as_ref().and_then(|item| item.state);
        match (entry.enumerated_kind.clone(), state) {
            (_, Some(SampleObservationState::AccessDenied)) => {
                Err(error(PortErrorKind::PermissionDenied, "os.query_failed"))
            }
            (_, Some(SampleObservationState::Vanished)) => {
                Err(error(PortErrorKind::NotFound, "os.not_found"))
            }
            (ObservedKind::Directory, _) => Ok(Box::new(Self::new(Arc::clone(&self.tree), child))),
            _ => Err(error(PortErrorKind::Unsupported, "os.unsupported")),
        }
    }

    fn observed_modified_at(&self) -> Option<Timestamp> {
        self.tree.nodes[self.node]
            .item
            .as_ref()
            .and_then(|item| item.modified_at.clone())
    }

    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        let node = &self.tree.nodes[self.node];
        // The sample root plays the role of a volume root: a confirmed empty ancestor list.
        Some(if self.node == ROOT {
            &[]
        } else {
            node.ancestors.as_slice()
        })
    }
}
