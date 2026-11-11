//! Minimize scope-owner-validated refresh directories without doing filesystem I/O.
//!
//! The owner must resolve target identity, scope boundaries, exclusions, volume and
//! mount policy before constructing these inputs. Components are comparison-key
//! components relative to that exact included target; they are not display paths.
//! An empty component vector identifies the included target root. This module does
//! not infer identity or verify that its inputs are safe to scan.

use std::collections::BTreeMap;

/// A directory already admitted by the owner of one pinned scope version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckedDirectory<T, C> {
    pub(crate) target: T,
    pub(crate) components: Vec<C>,
    pub(crate) include_descendants: bool,
}

/// The minimal selected directories for an included target, or the whole target.
/// A whole target still requires its owner to apply the scope's exclusions and
/// boundary rules during collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TargetSelection<C> {
    Directories(Vec<Directory<C>>),
    WholeTarget,
}

/// A directory comparison key relative to its included target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Directory<C> {
    pub(crate) components: Vec<C>,
    pub(crate) include_descendants: bool,
}

/// Minimize checked inputs and promote each target independently above `limit`.
///
/// `T` must uniquely identify an included target within the same scope version.
/// `C` must preserve the scope owner's component-wise comparison equivalence and
/// ordering. The supplied limit is applied after deduplication and absorption;
/// exactly `limit` directories stay explicit, including when the limit is zero.
pub(crate) fn minimize<T: Ord, C: Ord>(
    mut inputs: Vec<CheckedDirectory<T, C>>,
    limit: usize,
) -> BTreeMap<T, TargetSelection<C>> {
    inputs.sort_unstable_by(|a, b| {
        a.target
            .cmp(&b.target)
            .then_with(|| a.components.cmp(&b.components))
    });
    let mut grouped: BTreeMap<T, Vec<Directory<C>>> = BTreeMap::new();
    for input in inputs {
        let directories = grouped.entry(input.target).or_default();
        if let Some(previous) = directories.last_mut() {
            if previous.components == input.components {
                previous.include_descendants |= input.include_descendants;
                continue;
            }
            if previous.include_descendants && input.components.starts_with(&previous.components) {
                continue;
            }
        }
        directories.push(Directory {
            components: input.components,
            include_descendants: input.include_descendants,
        });
    }
    grouped
        .into_iter()
        .map(|(target, directories)| {
            let selection = if directories.len() > limit {
                TargetSelection::WholeTarget
            } else {
                TargetSelection::Directories(directories)
            };
            (target, selection)
        })
        .collect()
}

#[cfg(test)]
mod tests;
