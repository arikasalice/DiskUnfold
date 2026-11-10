//! One-volume Sankey attribution over already prepared, disjoint source rows.
//!
//! This is a pure calculation unit. It does not establish F03 attribution,
//! produce IPC models, select categories, or turn space ownership into movement.
pub mod geometry_v1;

use super::geometry::{Rect, Weight};
use std::collections::{BTreeMap, HashSet};

/// One already-attributed row. Negative values are rejected as flow inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub folder: u32,
    /// Caller-established deterministic folder tie order.
    pub folder_order: u32,
    pub category: u32,
    /// `None` means the source could not establish an amount.
    pub bytes: Option<i128>,
}

/// Three ordered non-overlapping node columns using one common vertical extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Columns {
    pub volume: Rect,
    pub folders: Rect,
    pub categories: Rect,
}

/// A real folder, the explicitly grouped tail, or the unattributed remainder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FolderKey {
    Folder(u32),
    Other,
    Unattributed,
}

/// A caller-supplied category or the unattributed remainder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CategoryKey {
    Category(u32),
    Unattributed,
}

/// A node's exact amount and its rectangle, if representable on the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node<K> {
    pub key: K,
    pub bytes: u64,
    pub rect: Option<Rect>,
    /// Nonempty only for Other, in the validated folder ranking order.
    pub members: Vec<u32>,
}

/// Exact nonnegative flow from one returned folder to one returned category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub folder: FolderKey,
    pub category: CategoryKey,
    pub bytes: u64,
}

/// A conserved one-volume graph. Link band geometry is deliberately not inferred.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graph {
    pub total: u64,
    pub volume: Node<()>,
    pub folders: Vec<Node<FolderKey>>,
    pub categories: Vec<Node<CategoryKey>>,
    pub links: Vec<Link>,
    pub unattributed: u64,
}

/// The requested graph cannot be established from these source facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    NoAttribution,
    NegativeFlow,
    UncertainScope,
    UnknownAttribution,
    Overflow,
}

/// Invalid caller inputs, distinct from unavailable source data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    Columns,
    FolderBudget,
    FolderOrder,
    CategoryOrder,
    ExceedsScope,
}

/// The graph is either conserved, unavailable, or rejected for invalid input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Graph(Graph),
    Unavailable(Unavailable),
    Invalid(Invalid),
}

#[derive(Default)]
struct Folder {
    order: u32,
    total: u64,
    categories: BTreeMap<u32, u64>,
}

/// Builds one volume → folders → categories graph from exact attributed rows.
///
/// `folder_budget` bounds attributed folder nodes including Other; the separate
/// unattributed node is retained when needed. Ties use `folder_order`, which
/// must be unique per folder and supplied by the caller. Category nodes follow
/// `category_order`. Unknown or lower-bound scope totals, unknown row amounts,
/// negative flows, and overflows never become zero-valued flows.
pub fn layout(
    columns: Columns,
    scope: Weight,
    rows: &[Row],
    folder_budget: usize,
    category_order: &[u32],
) -> Outcome {
    if !valid_columns(columns) {
        return Outcome::Invalid(Invalid::Columns);
    }
    let total = match scope {
        Weight::Exact(value) => value,
        _ => return Outcome::Unavailable(Unavailable::UncertainScope),
    };
    if rows.is_empty() {
        return Outcome::Unavailable(Unavailable::NoAttribution);
    }
    if folder_budget == 0 {
        return Outcome::Invalid(Invalid::FolderBudget);
    }
    let category_set: HashSet<_> = category_order.iter().copied().collect();
    if category_set.len() != category_order.len() {
        return Outcome::Invalid(Invalid::CategoryOrder);
    }
    let mut folders = BTreeMap::<u32, Folder>::new();
    let mut orders = BTreeMap::<u32, u32>::new();
    let mut attributed = 0u64;
    for row in rows {
        let Some(bytes) = row.bytes else {
            return Outcome::Unavailable(Unavailable::UnknownAttribution);
        };
        if bytes < 0 {
            return Outcome::Unavailable(Unavailable::NegativeFlow);
        }
        let Ok(bytes) = u64::try_from(bytes) else {
            return Outcome::Unavailable(Unavailable::Overflow);
        };
        if !category_set.contains(&row.category) {
            return Outcome::Invalid(Invalid::CategoryOrder);
        }
        if orders
            .get(&row.folder_order)
            .is_some_and(|&folder| folder != row.folder)
        {
            return Outcome::Invalid(Invalid::FolderOrder);
        }
        orders.insert(row.folder_order, row.folder);
        let folder = folders.entry(row.folder).or_insert_with(|| Folder {
            order: row.folder_order,
            ..Folder::default()
        });
        if folder.order != row.folder_order {
            return Outcome::Invalid(Invalid::FolderOrder);
        }
        let Some(sum) = folder.total.checked_add(bytes) else {
            return Outcome::Unavailable(Unavailable::Overflow);
        };
        folder.total = sum;
        let item = folder.categories.entry(row.category).or_default();
        let Some(sum) = item.checked_add(bytes) else {
            return Outcome::Unavailable(Unavailable::Overflow);
        };
        *item = sum;
        let Some(sum) = attributed.checked_add(bytes) else {
            return Outcome::Unavailable(Unavailable::Overflow);
        };
        attributed = sum;
    }
    if attributed > total {
        return Outcome::Invalid(Invalid::ExceedsScope);
    }
    let remainder = total - attributed;
    let mut ranked: Vec<_> = folders.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.total
            .cmp(&a.1.total)
            .then_with(|| a.1.order.cmp(&b.1.order))
    });
    make_graph(
        columns,
        total,
        remainder,
        ranked,
        folder_budget,
        category_order,
    )
}

fn make_graph(
    columns: Columns,
    total: u64,
    remainder: u64,
    ranked: Vec<(u32, Folder)>,
    budget: usize,
    category_order: &[u32],
) -> Outcome {
    let cutoff = if ranked.len() > budget {
        budget - 1
    } else {
        ranked.len()
    };
    let mut grouped = BTreeMap::<u32, u64>::new();
    let mut category_totals = BTreeMap::<u32, u64>::new();
    let mut folder_values = Vec::new();
    let mut link_values = Vec::new();
    let mut members = Vec::new();
    let mut other_total = 0u64;
    for (index, (id, folder)) in ranked.into_iter().enumerate() {
        let key = if index < cutoff {
            FolderKey::Folder(id)
        } else {
            members.push(id);
            other_total += folder.total;
            FolderKey::Other
        };
        if key != FolderKey::Other {
            folder_values.push((key, folder.total, Vec::new()));
        }
        for (category, bytes) in folder.categories {
            *category_totals.entry(category).or_default() += bytes;
            if key == FolderKey::Other {
                *grouped.entry(category).or_default() += bytes;
            } else {
                link_values.push(Link {
                    folder: key,
                    category: CategoryKey::Category(category),
                    bytes,
                });
            }
        }
    }
    if !members.is_empty() {
        folder_values.push((FolderKey::Other, other_total, members));
        for (category, bytes) in grouped {
            link_values.push(Link {
                folder: FolderKey::Other,
                category: CategoryKey::Category(category),
                bytes,
            });
        }
    }
    if remainder > 0 {
        folder_values.push((FolderKey::Unattributed, remainder, Vec::new()));
        link_values.push(Link {
            folder: FolderKey::Unattributed,
            category: CategoryKey::Unattributed,
            bytes: remainder,
        });
    }
    let mut category_values = Vec::new();
    for key in category_order {
        if let Some(bytes) = category_totals.remove(key) {
            category_values.push((CategoryKey::Category(*key), bytes, Vec::new()));
        }
    }
    if remainder > 0 {
        category_values.push((CategoryKey::Unattributed, remainder, Vec::new()));
    }
    let folders = nodes(columns.folders, total, folder_values);
    let categories = nodes(columns.categories, total, category_values);
    let mut positions = BTreeMap::new();
    for (index, node) in folders.iter().enumerate() {
        positions.insert(node.key, index);
    }
    let mut category_positions = BTreeMap::new();
    for (index, node) in categories.iter().enumerate() {
        category_positions.insert(node.key, index);
    }
    link_values.sort_by_key(|link| (positions[&link.folder], category_positions[&link.category]));
    Outcome::Graph(Graph {
        total,
        volume: Node {
            key: (),
            bytes: total,
            rect: (total > 0).then_some(columns.volume),
            members: Vec::new(),
        },
        folders,
        categories,
        links: link_values,
        unattributed: remainder,
    })
}

fn nodes<K>(column: Rect, total: u64, values: Vec<(K, u64, Vec<u32>)>) -> Vec<Node<K>> {
    let mut previous = 0u32;
    let mut cumulative = 0u64;
    values
        .into_iter()
        .map(|(key, bytes, members)| {
            cumulative += bytes;
            let edge = if total == 0 {
                0
            } else {
                (u128::from(column.bottom - column.top) * u128::from(cumulative)
                    / u128::from(total)) as u32
            };
            let rect = (edge > previous).then_some(Rect {
                left: column.left,
                right: column.right,
                top: column.top + previous,
                bottom: column.top + edge,
            });
            previous = edge;
            Node {
                key,
                bytes,
                rect,
                members,
            }
        })
        .collect()
}

fn valid_columns(c: Columns) -> bool {
    c.volume.left < c.volume.right
        && c.folders.left < c.folders.right
        && c.categories.left < c.categories.right
        && c.volume.top < c.volume.bottom
        && c.volume.top == c.folders.top
        && c.volume.top == c.categories.top
        && c.volume.bottom == c.folders.bottom
        && c.volume.bottom == c.categories.bottom
        && c.volume.right <= c.folders.left
        && c.folders.right <= c.categories.left
}
