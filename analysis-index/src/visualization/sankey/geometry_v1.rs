//! Checked Sankey relation geometry over an already attributed source graph.
use super::{CategoryKey, FolderKey, Graph, Node};
use crate::visualization::geometry::Rect;
use du_core_contracts::visualization::geometry::{Geometry, GeometryError, Point};
use std::collections::{BTreeMap, BTreeSet};

/// An endpoint identified only within the source graph; this is not a model ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Volume,
    Folder(FolderKey),
    Category(CategoryKey),
}

/// A relation retains its exact source amount and graph-local endpoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relation {
    pub from: Endpoint,
    pub to: Endpoint,
    pub bytes: u64,
    /// Exact zero has no polygon; positive relations always have one.
    pub geometry: Option<Geometry>,
}

/// Both stages in source-node then target-node display order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relations {
    pub volume_to_folder: Vec<Relation>,
    pub folder_to_category: Vec<Relation>,
}

/// Failure never returns a partial graph; the source graph remains available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Cooperative cancellation interrupted validation or geometry creation.
    Cancelled,
    /// Endpoint identity, uniqueness, or exact byte conservation failed.
    InvalidGraph,
    /// A positive slot has no height or the two node columns have no gap.
    Unrepresentable,
    /// The existing rectangle or polygon violates common geometry v1.
    InvalidGeometry(GeometryError),
}

impl Graph {
    /// Produces closed connection polygons without altering source data.
    ///
    /// The input must be a complete graph from the same pinned result and its
    /// rectangles must already use normalized coordinates. Slots follow each
    /// opposite node's display order. An unrepresentable positive connection
    /// fails the entire conversion; callers retain this graph for a table.
    pub fn geometry_v1(&self) -> Result<Relations, Error> {
        self.geometry_v1_cancellable(&mut || false)
    }

    /// Produces geometry with cancellation checks during graph traversal.
    pub fn geometry_v1_cancellable(
        &self,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Relations, Error> {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let index = Index::validate(self, cancelled)?;
        let first = volume_relations(self, cancelled)?;
        let second = category_relations(self, &index, cancelled)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(Relations {
            volume_to_folder: first,
            folder_to_category: second,
        })
    }
}

struct Index {
    folders: BTreeMap<FolderKey, usize>,
    categories: BTreeMap<CategoryKey, usize>,
}
impl Index {
    fn validate(graph: &Graph, cancelled: &mut dyn FnMut() -> bool) -> Result<Self, Error> {
        if graph.volume.bytes != graph.total {
            return Err(Error::InvalidGraph);
        }
        validate_node(&graph.volume)?;
        let mut folders = BTreeMap::new();
        let mut categories = BTreeMap::new();
        for (i, node) in graph.folders.iter().enumerate() {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            validate_node(node)?;
            if folders.insert(node.key, i).is_some() {
                return Err(Error::InvalidGraph);
            }
        }
        for (i, node) in graph.categories.iter().enumerate() {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            validate_node(node)?;
            if categories.insert(node.key, i).is_some() {
                return Err(Error::InvalidGraph);
            }
        }
        let mut folder_sum = vec![0_u64; folders.len()];
        let mut category_sum = vec![0_u64; categories.len()];
        let mut pairs = BTreeSet::new();
        for link in &graph.links {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let f = *folders.get(&link.folder).ok_or(Error::InvalidGraph)?;
            let c = *categories.get(&link.category).ok_or(Error::InvalidGraph)?;
            if !pairs.insert((f, c)) {
                return Err(Error::InvalidGraph);
            }
            folder_sum[f] = folder_sum[f]
                .checked_add(link.bytes)
                .ok_or(Error::InvalidGraph)?;
            category_sum[c] = category_sum[c]
                .checked_add(link.bytes)
                .ok_or(Error::InvalidGraph)?;
        }
        for (node, sum) in graph.folders.iter().zip(&folder_sum) {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            if node.bytes != *sum {
                return Err(Error::InvalidGraph);
            }
        }
        for (node, sum) in graph.categories.iter().zip(&category_sum) {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            if node.bytes != *sum {
                return Err(Error::InvalidGraph);
            }
        }
        if sum_nodes(&graph.folders, cancelled)? != graph.total
            || sum_nodes(&graph.categories, cancelled)? != graph.total
        {
            return Err(Error::InvalidGraph);
        }
        Ok(Self {
            folders,
            categories,
        })
    }
}
fn sum_nodes<K>(nodes: &[Node<K>], cancelled: &mut dyn FnMut() -> bool) -> Result<u64, Error> {
    let mut sum = 0_u64;
    for node in nodes {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        sum = sum.checked_add(node.bytes).ok_or(Error::InvalidGraph)?;
    }
    Ok(sum)
}
fn validate_node<K>(node: &Node<K>) -> Result<(), Error> {
    if let Some(rect) = node.rect {
        Geometry::new(
            "rect",
            vec![
                Point {
                    x: rect.left,
                    y: rect.top,
                },
                Point {
                    x: rect.right,
                    y: rect.bottom,
                },
            ],
        )
        .map_err(Error::InvalidGeometry)?;
    }
    Ok(())
}

fn volume_relations(
    graph: &Graph,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<Relation>, Error> {
    let mut used = 0;
    let mut out = Vec::with_capacity(graph.folders.len());
    for folder in &graph.folders {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let geometry = band(&graph.volume, folder, &mut used, &mut 0, folder.bytes)?;
        out.push(Relation {
            from: Endpoint::Volume,
            to: Endpoint::Folder(folder.key),
            bytes: folder.bytes,
            geometry,
        });
    }
    Ok(out)
}

fn category_relations(
    graph: &Graph,
    index: &Index,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<Relation>, Error> {
    let mut links: Vec<_> = graph.links.iter().collect();
    links.sort_by_key(|link| {
        (
            index.folders[&link.folder],
            index.categories[&link.category],
        )
    });
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let mut folder_used = vec![0; graph.folders.len()];
    let mut category_used = vec![0; graph.categories.len()];
    let mut out = Vec::with_capacity(links.len());
    for link in links {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let f = index.folders[&link.folder];
        let c = index.categories[&link.category];
        let geometry = band(
            &graph.folders[f],
            &graph.categories[c],
            &mut folder_used[f],
            &mut category_used[c],
            link.bytes,
        )?;
        out.push(Relation {
            from: Endpoint::Folder(link.folder),
            to: Endpoint::Category(link.category),
            bytes: link.bytes,
            geometry,
        });
    }
    Ok(out)
}

fn band<A, B>(
    source: &Node<A>,
    target: &Node<B>,
    source_used: &mut u64,
    target_used: &mut u64,
    bytes: u64,
) -> Result<Option<Geometry>, Error> {
    if bytes == 0 {
        return Ok(None);
    }
    let (source_rect, target_rect) = (
        source.rect.ok_or(Error::Unrepresentable)?,
        target.rect.ok_or(Error::Unrepresentable)?,
    );
    if source_rect.right >= target_rect.left {
        return Err(Error::Unrepresentable);
    }
    let (source_top, source_bottom) = slot(source_rect, source.bytes, source_used, bytes)?;
    let (target_top, target_bottom) = slot(target_rect, target.bytes, target_used, bytes)?;
    let points = vec![
        Point {
            x: source_rect.right,
            y: source_top,
        },
        Point {
            x: target_rect.left,
            y: target_top,
        },
        Point {
            x: target_rect.left,
            y: target_bottom,
        },
        Point {
            x: source_rect.right,
            y: source_bottom,
        },
    ];
    Geometry::new("polygon", points)
        .map(Some)
        .map_err(Error::InvalidGeometry)
}
fn slot(rect: Rect, total: u64, used: &mut u64, bytes: u64) -> Result<(u32, u32), Error> {
    let end = used
        .checked_add(bytes)
        .filter(|end| *end <= total)
        .ok_or(Error::InvalidGraph)?;
    if total == 0 {
        return Err(Error::InvalidGraph);
    }
    let height = u128::from(rect.bottom - rect.top);
    let edge = |value| rect.top + (height * u128::from(value) / u128::from(total)) as u32;
    let start = edge(*used);
    let finish = edge(end);
    if start == finish {
        return Err(Error::Unrepresentable);
    }
    *used = end;
    Ok((start, finish))
}
