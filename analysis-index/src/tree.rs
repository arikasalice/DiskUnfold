//! Deterministic forest numbering and hierarchy construction.
use crate::{BuildError, SpaceObservation, StoredObservation};
use du_core_contracts::Timestamp;
use du_core_contracts::result::{ResultItem, ResultItemKind};
use std::collections::BTreeMap;

pub(crate) struct BuiltItems {
    pub(crate) items: Vec<ResultItem>,
    pub(crate) structures: Vec<bool>,
    pub(crate) physical: Option<Vec<Option<SpaceObservation>>>,
    pub(crate) modified_times: Option<Vec<Option<Timestamp>>>,
    pub(crate) native_modified_times:
        Option<Vec<Option<du_core_contracts::scan_observation::NativeModifiedTime>>>,
    pub(crate) enumerated_inodes: Option<Vec<Option<std::num::NonZeroU64>>>,
    pub(crate) item_observed_times: Option<Vec<Option<Timestamp>>>,
    pub(crate) keys: Vec<(u32, Vec<Vec<u8>>)>,
}

pub(crate) fn build_items(
    observations: &BTreeMap<(u32, Vec<Vec<u8>>), StoredObservation>,
    root_count: usize,
    strict_roots: bool,
) -> Result<BuiltItems, BuildError> {
    u32::try_from(observations.len()).map_err(|_| BuildError::CapacityExceeded)?;
    let mut by_parent: BTreeMap<(u32, Vec<Vec<u8>>), Vec<_>> = BTreeMap::new();
    for ((root, path), observation) in observations
        .iter()
        .filter(|((_, path), _)| !path.is_empty())
    {
        by_parent
            .entry((*root, path[..path.len() - 1].to_vec()))
            .or_default()
            .push((path, observation));
    }
    let mut items = Vec::new();
    let mut paths = Vec::new();
    let mut structure_flags = Vec::new();
    let mut physical_values = observations
        .values()
        .any(|v| v.physical.is_some())
        .then(Vec::new);
    let mut modified_times = observations
        .values()
        .any(|v| v.modified_at.is_some())
        .then(Vec::new);
    let mut native_modified_times = observations
        .values()
        .any(|v| v.native_modified_at.is_some())
        .then(Vec::new);
    let mut item_observed_times = observations
        .values()
        .any(|v| v.observed_at.is_some())
        .then(Vec::new);
    let mut enumerated_inodes = observations
        .values()
        .any(|v| v.enumerated_inode.is_some())
        .then(Vec::new);
    for index in 0..root_count {
        let root_index = u32::try_from(index).map_err(|_| BuildError::CapacityExceeded)?;
        let root = observations
            .get(&(root_index, Vec::new()))
            .ok_or(BuildError::InvalidTree)?;
        if strict_roots && root.kind != ResultItemKind::Directory {
            return Err(BuildError::InvalidTree);
        }
        items.push(ResultItem {
            item_no: root_index,
            parent: None,
            name: Vec::new(),
            size: root.size.clone(),
            kind: root.kind,
        });
        paths.push((root_index, Vec::new()));
        structure_flags.push(is_structure(root.kind));
        if let Some(values) = &mut physical_values {
            values.push(root.physical.as_deref().cloned());
        }
        if let Some(values) = &mut modified_times {
            values.push(root.modified_at.clone());
        }
        if let Some(values) = &mut native_modified_times {
            values.push(root.native_modified_at);
        }
        if let Some(values) = &mut enumerated_inodes {
            values.push(root.enumerated_inode);
        }
        if let Some(values) = &mut item_observed_times {
            values.push(root.observed_at.clone());
        }
    }
    let mut cursor = 0;
    while cursor < paths.len() {
        let (root_index, path) = &paths[cursor];
        let parent = u32::try_from(cursor).map_err(|_| BuildError::CapacityExceeded)?;
        let current_root = *root_index;
        for (child, observation) in by_parent
            .remove(&(current_root, path.clone()))
            .unwrap_or_default()
        {
            let item_no = u32::try_from(items.len()).map_err(|_| BuildError::CapacityExceeded)?;
            items.push(ResultItem {
                item_no,
                parent: Some(parent),
                name: child.last().cloned().ok_or(BuildError::InvalidTree)?,
                size: observation.size.clone(),
                kind: observation.kind,
            });
            paths.push((current_root, child.clone()));
            structure_flags.push(is_structure(observation.kind));
            if let Some(values) = &mut physical_values {
                values.push(observation.physical.as_deref().cloned());
            }
            if let Some(values) = &mut modified_times {
                values.push(observation.modified_at.clone());
            }
            if let Some(values) = &mut native_modified_times {
                values.push(observation.native_modified_at);
            }
            if let Some(values) = &mut enumerated_inodes {
                values.push(observation.enumerated_inode);
            }
            if let Some(values) = &mut item_observed_times {
                values.push(observation.observed_at.clone());
            }
        }
        cursor += 1;
    }
    if items.len() != observations.len() {
        return Err(BuildError::InvalidTree);
    }
    Ok(BuiltItems {
        items,
        structures: structure_flags,
        physical: physical_values,
        modified_times,
        native_modified_times,
        enumerated_inodes,
        item_observed_times,
        keys: paths,
    })
}

fn is_structure(kind: ResultItemKind) -> bool {
    matches!(
        kind,
        ResultItemKind::Directory | ResultItemKind::UnknownStructure
    )
}
