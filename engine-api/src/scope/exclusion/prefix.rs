//! Frozen component prefixes for path exclusion, indexed per retained root.

use du_core_contracts::scan_observation::FilterDecision;
use std::collections::BTreeMap;

#[derive(Default)]
struct Node {
    exclude_subtree: bool,
    children: BTreeMap<Vec<u8>, usize>,
}

/// Flat nodes make rule construction and destruction safe for deeply nested paths.
pub(super) struct PrefixFilter(Vec<Node>);

impl PrefixFilter {
    pub(super) fn new<'a>(rules: impl IntoIterator<Item = &'a [Vec<u8>]>) -> Self {
        let mut nodes = vec![Node::default()];
        for rule in rules {
            let mut current = 0;
            for key in rule {
                let next = if let Some(&child) = nodes[current].children.get(key) {
                    child
                } else {
                    let next = nodes.len();
                    nodes[current].children.insert(key.clone(), next);
                    nodes.push(Node::default());
                    next
                };
                current = next;
            }
            nodes[current].exclude_subtree = true;
        }
        Self(nodes)
    }

    /// Examines only prefixes that could still match an exclusion.
    ///
    /// An unknown comparison on such a prefix is undecidable. Once every rule
    /// diverges, an unrelated deeper name cannot invalidate this decision.
    pub(super) fn decide(
        &self,
        parent: &[Vec<u8>],
        name: &[u8],
        mut key: impl FnMut(usize, &[u8]) -> Option<Vec<u8>>,
    ) -> FilterDecision {
        let mut current = &self.0[0];
        for (index, raw) in parent
            .iter()
            .map(Vec::as_slice)
            .chain(std::iter::once(name))
            .enumerate()
        {
            if current.exclude_subtree {
                return FilterDecision::Exclude;
            }
            if current.children.is_empty() {
                return FilterDecision::Admit;
            }
            let Some(component) = key(index, raw) else {
                return FilterDecision::Undecidable;
            };
            let Some(&child) = current.children.get(&component) else {
                return FilterDecision::Admit;
            };
            current = &self.0[child];
        }
        if current.exclude_subtree {
            FilterDecision::Exclude
        } else {
            FilterDecision::Admit
        }
    }
}
