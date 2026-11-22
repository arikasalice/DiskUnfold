//! Compact physical observation records and their byte arenas.

use super::{FileIdentity, SharingEvidence, SpaceObservation};
use crate::BuildError;
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, SizeValue};
use std::collections::HashMap;

pub(super) const CHUNK_BITS: u32 = 12;
pub(super) const CHUNK_MASK: u32 = (1 << CHUNK_BITS) - 1;
pub(super) const HAS_OBSERVATION: u32 = 1 << 0;
pub(super) const HAS_ALLOCATION: u32 = 1 << 1;
pub(super) const HAS_IDENTITY: u32 = 1 << 2;
pub(super) const INLINE_IDENTITY: u32 = 1 << 3;
pub(super) const HAS_LINK_COUNT: u32 = 1 << 4;
pub(super) const SHARING_UNKNOWN: u32 = 1 << 5;
pub(super) const INLINE_FILE_IDENTITY: u32 = 1 << 6;
const MAX_INTERNED_VOLUMES: usize = 256;

#[derive(Clone, Debug)]
pub(crate) struct PhysicalRecord {
    pub(super) flags: u32,
    pub(super) allocated: SizeValue,
    pub(super) link_count: u64,
    pub(super) identity: [u8; 16],
}

impl Default for PhysicalRecord {
    fn default() -> Self {
        Self {
            flags: 0,
            allocated: SizeValue {
                basis: SizeBasis::Allocated,
                evidence: SizeEvidence::Unknown,
                amount: SizeAmount::Unknown,
            },
            link_count: 0,
            identity: [0; 16],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct PhysicalChunk {
    records: Vec<(u16, PhysicalRecord)>,
    arena: Vec<u8>,
    volumes: HashMap<Vec<u8>, (u32, u32)>,
}

impl PhysicalChunk {
    pub(super) fn push(
        &mut self,
        node_offset: u16,
        observation: &SpaceObservation,
    ) -> Result<(), BuildError> {
        if self
            .records
            .last()
            .is_some_and(|(last, _)| *last >= node_offset)
        {
            return Err(BuildError::InvalidTree);
        }
        let record = encode(observation, &mut self.arena, &mut self.volumes)?;
        self.records.push((node_offset, record));
        Ok(())
    }

    pub(super) fn record(&self, node_offset: u16) -> Option<&PhysicalRecord> {
        let index = self
            .records
            .binary_search_by_key(&node_offset, |(offset, _)| *offset)
            .ok()?;
        Some(&self.records[index].1)
    }

    pub(super) fn arena(&self) -> &[u8] {
        &self.arena
    }

    pub(super) fn clear_builder_index(&mut self) {
        self.volumes = HashMap::new();
    }
}

pub(super) fn encode(
    observation: &SpaceObservation,
    arena: &mut Vec<u8>,
    volumes: &mut HashMap<Vec<u8>, (u32, u32)>,
) -> Result<PhysicalRecord, BuildError> {
    let mut record = PhysicalRecord {
        flags: HAS_OBSERVATION,
        ..PhysicalRecord::default()
    };
    if let Some(value) = &observation.allocated {
        record.flags |= HAS_ALLOCATION;
        record.allocated = value.clone();
    }
    if let Some(identity) = &observation.identity {
        record.flags |= HAS_IDENTITY;
        if identity.volume.len() == 8 && identity.file.len() == 8 {
            record.flags |= INLINE_IDENTITY;
            record.identity[..8].copy_from_slice(&identity.volume);
            record.identity[8..].copy_from_slice(&identity.file);
        } else if identity.file.len() == 8 {
            if let Some((offset, length)) = intern_volume(&identity.volume, arena, volumes) {
                record.flags |= INLINE_FILE_IDENTITY;
                record.identity[..8].copy_from_slice(&identity.file);
                record.identity[8..12].copy_from_slice(&offset.to_le_bytes());
                record.identity[12..].copy_from_slice(&length.to_le_bytes());
            } else {
                encode_arena_identity(&mut record, identity, arena)?;
            }
        } else {
            encode_arena_identity(&mut record, identity, arena)?;
        }
    }
    if let Some(link_count) = observation.link_count {
        record.flags |= HAS_LINK_COUNT;
        record.link_count = link_count;
    }
    if observation.sharing == SharingEvidence::Unknown {
        record.flags |= SHARING_UNKNOWN;
    }
    Ok(record)
}

fn intern_volume(
    volume: &[u8],
    arena: &mut Vec<u8>,
    volumes: &mut HashMap<Vec<u8>, (u32, u32)>,
) -> Option<(u32, u32)> {
    if let Some(location) = volumes.get(volume) {
        return Some(*location);
    }
    if volumes.len() >= MAX_INTERNED_VOLUMES {
        return None;
    }
    let offset = u32::try_from(arena.len()).ok()?;
    let length = u32::try_from(volume.len()).ok()?;
    let end = usize::try_from(offset).ok()?.checked_add(volume.len())?;
    if end > u32::MAX as usize {
        return None;
    }
    arena.extend_from_slice(volume);
    volumes.insert(volume.to_vec(), (offset, length));
    Some((offset, length))
}

fn encode_arena_identity(
    record: &mut PhysicalRecord,
    identity: &FileIdentity,
    arena: &mut Vec<u8>,
) -> Result<(), BuildError> {
    let offset = u64::try_from(arena.len()).map_err(|_| BuildError::CapacityExceeded)?;
    let volume_len =
        u64::try_from(identity.volume.len()).map_err(|_| BuildError::CapacityExceeded)?;
    let file_len = u64::try_from(identity.file.len()).map_err(|_| BuildError::CapacityExceeded)?;
    record.identity[..8].copy_from_slice(&offset.to_le_bytes());
    arena.extend_from_slice(&volume_len.to_le_bytes());
    arena.extend_from_slice(&file_len.to_le_bytes());
    arena.extend_from_slice(&identity.volume);
    arena.extend_from_slice(&identity.file);
    Ok(())
}
