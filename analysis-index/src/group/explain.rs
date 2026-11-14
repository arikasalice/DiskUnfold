//! Per-rule explanation of one file's decisions.

use du_core_contracts::{
    ItemRef,
    result::{ResultItem, ResultItemKind, ResultVersionRead},
};

use super::{Classification, Decision, Error};
use crate::query::{self, Record, Registry};

/// The decision of one condition node. `path` lists child indices from the rule root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeDecision {
    /// Child indices from the rule's condition root; the root's path is empty.
    pub path: Vec<u32>,
    /// Predicate ID and revision when the node is a leaf.
    pub predicate: Option<(String, u32)>,
    /// The node's three-valued decision, or unavailable when its subtree holds an
    /// unregistered predicate.
    pub decision: Decision,
}

/// One rule's decision for the file and the decision of each node of its condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleExplanation {
    /// The rule's decision; it equals the decision stored by the classification.
    pub decision: Decision,
    /// Every node in pre-order: a node precedes its children, which follow condition order.
    pub nodes: Vec<NodeDecision>,
}

impl Classification {
    /// Evaluates one member file again with the classified rules, recording every node.
    ///
    /// Nodes combine with the F04 three-valued logic used for classification, so an unknown
    /// leaf stays unknown and is never turned into a mismatch. `registry` supplies the leaf
    /// predicates; a rule that was unavailable at classification stays unavailable.
    ///
    /// # Errors
    /// Returns [`Error::InvalidItem`] for an item of another result version, an item that is
    /// not a file, or a file outside the classified scope.
    pub fn explain(
        &self,
        item: &ItemRef,
        registry: &Registry,
    ) -> Result<Vec<RuleExplanation>, Error> {
        let file = self.scope_file(item)?;
        let path = query::navigation_path(&self.version, file, None);
        let record = Record {
            item: file,
            path: &path,
            version: &self.version,
        };
        self.rules
            .iter()
            .zip(&self.rule_unavailable)
            .map(|(rule, &unavailable)| {
                let nodes = query::plan::explain(registry, rule, &record)
                    .map_err(Error::InvalidRule)?
                    .into_iter()
                    .map(|node| NodeDecision {
                        path: node.path,
                        predicate: node.predicate,
                        decision: node.truth.map_or(Decision::RuleUnavailable, Decision::from),
                    })
                    .collect::<Vec<_>>();
                let decision = match nodes.first() {
                    Some(root) if !unavailable => root.decision,
                    _ => Decision::RuleUnavailable,
                };
                Ok(RuleExplanation { decision, nodes })
            })
            .collect()
    }

    /// Resolves a reference to a file inside the pinned version and scope.
    fn scope_file(&self, item: &ItemRef) -> Result<&ResultItem, Error> {
        if &item.result_version != self.version.version_id() {
            return Err(Error::InvalidItem);
        }
        let file = self
            .version
            .items()
            .get(item.item_no as usize)
            .filter(|file| file.kind == ResultItemKind::File)
            .ok_or(Error::InvalidItem)?;
        match &self.root {
            Some(root) if !self.descends_from(file, root.item_no) => Err(Error::InvalidItem),
            _ => Ok(file),
        }
    }

    fn descends_from(&self, item: &ResultItem, ancestor: u32) -> bool {
        let mut current = item;
        while let Some(parent) = current.parent {
            if parent == ancestor {
                return true;
            }
            let Some(next) = self.version.items().get(parent as usize) else {
                return false;
            };
            current = next;
        }
        false
    }
}
