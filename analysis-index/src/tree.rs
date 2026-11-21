//! Deterministic forest numbering and hierarchy construction.
use crate::item_kind::is_directory;
use crate::observation_store::ObservationStore;
use crate::physical::{PhysicalObservations, PhysicalObservationsBuilder};
use crate::{BuildError, StoredObservation, observation::AttributeReasons};
use du_core_contracts::Timestamp;
use du_core_contracts::result::ResultItem;
use du_core_contracts::scan_observation::{NativeModifiedTime, PlaceholderObservation};
use std::sync::Arc;

pub(crate) struct BuiltItems {
    pub(crate) items: Vec<ResultItem>,
    pub(crate) physical: Option<PhysicalObservations>,
    pub(crate) modified_times: Option<Vec<Option<Timestamp>>>,
    pub(crate) native_modified_times: Option<Vec<Option<NativeModifiedTime>>>,
    pub(crate) enumerated_inodes: Option<Vec<Option<std::num::NonZeroU64>>>,
    pub(crate) placeholder_observations: Option<Vec<Option<PlaceholderObservation>>>,
    pub(crate) item_observed_times: Option<Vec<Option<Timestamp>>>,
    /// Root index of every item, in item order.
    pub(crate) root_indices: Vec<u32>,
    /// Item number of every store node in this publication.
    pub(crate) item_of_node: Vec<u32>,
    pub(crate) sparse: SparseItems,
}

impl BuiltItems {
    pub(crate) fn file_evidence(&self) -> crate::physical::FileEvidence<'_> {
        crate::physical::FileEvidence {
            observations: self.physical.as_ref(),
            placeholders: self.placeholder_observations.as_deref(),
        }
    }
}

/// Facts that only a few items carry, bound to the item numbers of one publication.
#[derive(Debug, Default)]
pub(crate) struct SparseItems {
    /// Files outside the scan boundary, ascending by item number.
    pub(crate) boundary: Box<[u32]>,
    /// Why attributes of items are unknown.
    pub(crate) attribute_reasons: AttributeReasons,
}

/// Numbers the observed forest: the roots in root order, then every item's
/// children after all earlier items, siblings in raw name byte order.
///
/// Fails with `InvalidTree` when a root is missing, a strict root is not a
/// directory, or a stored path names an ancestor that was never observed.
pub(crate) fn build_items(
    store: &ObservationStore,
    root_count: usize,
    strict_roots: bool,
    numbered_nodes: &mut Option<Arc<Vec<u32>>>,
) -> Result<BuiltItems, BuildError> {
    if store.has_unobserved() {
        return Err(BuildError::InvalidTree);
    }
    let order = match numbered_nodes {
        Some(order) => Arc::clone(order),
        None => {
            let order = Arc::new(number_nodes(store, root_count, strict_roots)?);
            *numbered_nodes = Some(Arc::clone(&order));
            order
        }
    };
    let mut item_of_node = vec![0_u32; order.len()];
    for (item, node) in order.iter().enumerate() {
        item_of_node[*node as usize] =
            u32::try_from(item).map_err(|_| BuildError::CapacityExceeded)?;
    }
    let mut items = Vec::with_capacity(order.len());
    let mut root_indices: Vec<u32> = Vec::with_capacity(order.len());
    let mut columns = Columns::for_store(store, order.len());
    for (item, node) in order.iter().enumerate() {
        let observation = store.observation(*node).ok_or(BuildError::InvalidTree)?;
        let item_no = u32::try_from(item).map_err(|_| BuildError::CapacityExceeded)?;
        let parent = store
            .parent(*node)
            .map(|parent| item_of_node[parent as usize]);
        // A parent is numbered before its children, so its root index is known.
        let root_index = parent.map_or(item_no, |parent| root_indices[parent as usize]);
        root_indices.push(root_index);
        items.push(ResultItem {
            item_no,
            parent,
            name: store.name(*node).to_vec(),
            size: observation.size.clone(),
            kind: observation.kind,
        });
        columns.push(item_no, observation)?;
    }
    let mut boundary: Vec<u32> = store
        .boundary_nodes()
        .iter()
        .map(|&node| item_of_node[node as usize])
        .collect();
    boundary.sort_unstable();
    let attribute_reasons = AttributeReasons::bind(&item_of_node, store.attribute_reasons());
    let physical = columns.physical.map(PhysicalObservationsBuilder::finish);
    Ok(BuiltItems {
        items,
        physical,
        modified_times: columns.modified_times,
        native_modified_times: columns.native_modified_times,
        enumerated_inodes: columns.enumerated_inodes,
        placeholder_observations: columns.placeholder_observations,
        item_observed_times: columns.item_observed_times,
        root_indices,
        item_of_node,
        sparse: SparseItems {
            boundary: boundary.into_boxed_slice(),
            attribute_reasons,
        },
    })
}

/// Store nodes in item order.
fn number_nodes(
    store: &ObservationStore,
    root_count: usize,
    strict_roots: bool,
) -> Result<Vec<u32>, BuildError> {
    let nodes = store.nodes() as usize;
    let (ends, mut children) = group_children(store);
    let mut order = Vec::with_capacity(nodes);
    for index in 0..root_count {
        let root_index = u32::try_from(index).map_err(|_| BuildError::CapacityExceeded)?;
        let node = store.root_node(root_index).ok_or(BuildError::InvalidTree)?;
        let root = store.observation(node).ok_or(BuildError::InvalidTree)?;
        if strict_roots && !is_directory(root.kind) {
            return Err(BuildError::InvalidTree);
        }
        order.push(node);
    }
    let mut cursor = 0;
    while cursor < order.len() {
        let parent = order[cursor] as usize;
        let start = if parent == 0 { 0 } else { ends[parent - 1] };
        let siblings = &mut children[start as usize..ends[parent] as usize];
        // Names are unique within one parent, so this order is total.
        siblings.sort_unstable_by(|a, b| store.name(*a).cmp(store.name(*b)));
        order.extend_from_slice(siblings);
        cursor += 1;
    }
    if order.len() != nodes {
        return Err(BuildError::InvalidTree);
    }
    Ok(order)
}

/// Groups the named nodes by parent node.
///
/// Returns `(ends, children)`: the children of parent `p` are
/// `children[ends[p - 1]..ends[p]]`, starting at zero for the first node.
fn group_children(store: &ObservationStore) -> (Vec<u32>, Vec<u32>) {
    let mut ends = vec![0_u32; store.nodes() as usize];
    for node in 0..store.nodes() {
        if let Some(parent) = store.parent(node) {
            ends[parent as usize] += 1;
        }
    }
    let mut total = 0_u32;
    for end in &mut ends {
        let count = *end;
        *end = total;
        total += count;
    }
    let mut children = vec![0_u32; total as usize];
    for node in 0..store.nodes() {
        if let Some(parent) = store.parent(node) {
            let end = &mut ends[parent as usize];
            children[*end as usize] = node;
            *end += 1;
        }
    }
    (ends, children)
}

/// Per-item evidence columns; a column exists only when some item has a value.
struct Columns {
    physical: Option<PhysicalObservationsBuilder>,
    modified_times: Option<Vec<Option<Timestamp>>>,
    native_modified_times: Option<Vec<Option<NativeModifiedTime>>>,
    enumerated_inodes: Option<Vec<Option<std::num::NonZeroU64>>>,
    placeholder_observations: Option<Vec<Option<PlaceholderObservation>>>,
    item_observed_times: Option<Vec<Option<Timestamp>>>,
}

impl Columns {
    fn for_store(store: &ObservationStore, items: usize) -> Self {
        let mut present = [false; 6];
        let mut physical_count = 0;
        for node in 0..store.nodes() {
            if let Some(value) = store.observation(node) {
                present[0] |= value.physical.is_some();
                physical_count += usize::from(value.physical.is_some());
                present[1] |= value.modified_at.is_some();
                present[2] |= value.native_modified_at.is_some();
                present[3] |= value.enumerated_inode.is_some();
                present[4] |= value.placeholder_observation.is_some();
                present[5] |= value.observed_at.is_some();
            }
        }
        Self {
            physical: present[0].then(|| PhysicalObservations::builder(items, physical_count)),
            modified_times: present[1].then(|| Vec::with_capacity(items)),
            native_modified_times: present[2].then(|| Vec::with_capacity(items)),
            enumerated_inodes: present[3].then(|| Vec::with_capacity(items)),
            placeholder_observations: present[4].then(|| Vec::with_capacity(items)),
            item_observed_times: present[5].then(|| Vec::with_capacity(items)),
        }
    }

    fn push(&mut self, item_no: u32, observation: &StoredObservation) -> Result<(), BuildError> {
        if let Some(values) = &mut self.physical {
            values.push(item_no, observation.physical.as_deref())?;
        }
        if let Some(values) = &mut self.modified_times {
            values.push(observation.modified_at.clone());
        }
        if let Some(values) = &mut self.native_modified_times {
            values.push(observation.native_modified_at);
        }
        if let Some(values) = &mut self.enumerated_inodes {
            values.push(observation.enumerated_inode);
        }
        if let Some(values) = &mut self.placeholder_observations {
            values.push(observation.placeholder_observation);
        }
        if let Some(values) = &mut self.item_observed_times {
            values.push(observation.observed_at.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
