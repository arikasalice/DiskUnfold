//! Compact, immutable per-item physical observations.

#[cfg(test)]
use super::record::encode;
use super::record::{
    CHUNK_BITS, CHUNK_MASK, HAS_ALLOCATION, HAS_IDENTITY, HAS_LINK_COUNT, HAS_OBSERVATION,
    INLINE_FILE_IDENTITY, INLINE_IDENTITY, PhysicalChunk, SHARING_UNKNOWN,
};
use super::{FileIdentity, SharingEvidence, SpaceObservation};
use crate::BuildError;
use du_core_contracts::SizeValue;
#[cfg(test)]
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence};
#[cfg(test)]
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub(crate) use super::record::PhysicalRecord;

#[derive(Clone, Copy, Debug)]
pub(crate) struct FileIdentityRef<'a> {
    pub(crate) volume: &'a [u8],
    pub(crate) file: &'a [u8],
}

impl FileIdentityRef<'_> {
    pub(crate) fn to_owned(self) -> FileIdentity {
        FileIdentity {
            volume: self.volume.to_vec(),
            file: self.file.to_vec(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum PhysicalObservationRef<'a> {
    Compact {
        record: &'a PhysicalRecord,
        arena: &'a [u8],
    },
}

impl<'a> PhysicalObservationRef<'a> {
    pub(crate) fn allocated(self) -> Option<&'a SizeValue> {
        match self {
            Self::Compact { record, .. } => {
                (record.flags & HAS_ALLOCATION != 0).then_some(&record.allocated)
            }
        }
    }

    pub(crate) fn identity(self) -> Option<FileIdentityRef<'a>> {
        match self {
            Self::Compact { record, arena } => {
                if record.flags & HAS_IDENTITY == 0 {
                    return None;
                }
                let (volume, file) = if record.flags & INLINE_IDENTITY != 0 {
                    (&record.identity[..8], &record.identity[8..])
                } else if record.flags & INLINE_FILE_IDENTITY != 0 {
                    let offset =
                        u32::from_le_bytes(record.identity[8..12].try_into().ok()?) as usize;
                    let volume_len =
                        u32::from_le_bytes(record.identity[12..16].try_into().ok()?) as usize;
                    let volume_end = offset.checked_add(volume_len)?;
                    (arena.get(offset..volume_end)?, &record.identity[..8])
                } else {
                    let offset =
                        usize::try_from(u64::from_le_bytes(record.identity[..8].try_into().ok()?))
                            .ok()?;
                    let volume_len_end = offset.checked_add(8)?;
                    let file_len_end = volume_len_end.checked_add(8)?;
                    let volume_len = usize::try_from(u64::from_le_bytes(
                        arena.get(offset..volume_len_end)?.try_into().ok()?,
                    ))
                    .ok()?;
                    let file_len = usize::try_from(u64::from_le_bytes(
                        arena.get(volume_len_end..file_len_end)?.try_into().ok()?,
                    ))
                    .ok()?;
                    let volume_end = file_len_end.checked_add(volume_len)?;
                    let file_end = volume_end.checked_add(file_len)?;
                    (
                        arena.get(file_len_end..volume_end)?,
                        arena.get(volume_end..file_end)?,
                    )
                };
                Some(FileIdentityRef { volume, file })
            }
        }
    }

    pub(crate) fn link_count(self) -> Option<u64> {
        match self {
            Self::Compact { record, .. } => {
                (record.flags & HAS_LINK_COUNT != 0).then_some(record.link_count)
            }
        }
    }

    pub(crate) fn sharing(self) -> SharingEvidence {
        match self {
            Self::Compact { record, .. } => {
                if record.flags & SHARING_UNKNOWN != 0 {
                    SharingEvidence::Unknown
                } else {
                    SharingEvidence::None
                }
            }
        }
    }

    pub(crate) fn to_owned(self) -> SpaceObservation {
        SpaceObservation {
            allocated: self.allocated().cloned(),
            identity: self.identity().map(FileIdentityRef::to_owned),
            link_count: self.link_count(),
            sharing: self.sharing(),
        }
    }
}

#[derive(Debug)]
enum Records {
    NodeChunks {
        chunks: Vec<Option<Arc<PhysicalChunk>>>,
        order: Arc<Vec<u32>>,
    },
    #[cfg(test)]
    Dense(Vec<PhysicalRecord>),
    #[cfg(test)]
    Sparse {
        item_numbers: Vec<u32>,
        records: Vec<PhysicalRecord>,
    },
}

#[derive(Debug)]
pub(crate) struct PhysicalObservations {
    records: Records,
    #[cfg(test)]
    arena: Vec<u8>,
    present_count: usize,
}

impl PhysicalObservations {
    pub(super) fn from_node_chunks(
        chunks: Vec<Option<Arc<PhysicalChunk>>>,
        order: Arc<Vec<u32>>,
        present_count: usize,
    ) -> Self {
        Self {
            records: Records::NodeChunks { chunks, order },
            #[cfg(test)]
            arena: Vec::new(),
            present_count,
        }
    }

    #[cfg(test)]
    pub(crate) fn builder(item_count: usize, present_count: usize) -> PhysicalObservationsBuilder {
        PhysicalObservationsBuilder::new(item_count, present_count)
    }

    pub(crate) fn get(&self, item_no: u32) -> Option<PhysicalObservationRef<'_>> {
        match &self.records {
            Records::NodeChunks { chunks, order } => {
                let node = *order.get(item_no as usize)?;
                let chunk = chunks.get((node >> CHUNK_BITS) as usize)?.as_ref()?;
                let record = chunk.record((node & CHUNK_MASK) as u16)?;
                (record.flags & HAS_OBSERVATION != 0).then_some(PhysicalObservationRef::Compact {
                    record,
                    arena: chunk.arena(),
                })
            }
            #[cfg(test)]
            Records::Dense(records) => {
                let record = records.get(item_no as usize)?;
                (record.flags & HAS_OBSERVATION != 0).then_some(PhysicalObservationRef::Compact {
                    record,
                    arena: &self.arena,
                })
            }
            #[cfg(test)]
            Records::Sparse {
                item_numbers,
                records,
            } => {
                let record = records.get(item_numbers.binary_search(&item_no).ok()?)?;
                (record.flags & HAS_OBSERVATION != 0).then_some(PhysicalObservationRef::Compact {
                    record,
                    arena: &self.arena,
                })
            }
        }
    }

    pub(crate) fn present_count(&self) -> usize {
        self.present_count
    }

    #[cfg(test)]
    pub(crate) fn is_compact(&self) -> bool {
        matches!(&self.records, Records::NodeChunks { .. })
    }

    #[cfg(test)]
    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        match (&self.records, &other.records) {
            (
                Records::NodeChunks { chunks: left, .. },
                Records::NodeChunks { chunks: right, .. },
            ) => left.iter().zip(right).any(|(a, b)| match (a, b) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }),
            _ => false,
        }
    }
}

#[cfg(test)]
pub(crate) fn compact_test_observations(
    values: Option<&[Option<Arc<SpaceObservation>>]>,
) -> Option<PhysicalObservations> {
    let values = values?;
    let present = values.iter().flatten().count();
    let mut builder = PhysicalObservations::builder(values.len(), present);
    for (item_no, value) in values.iter().enumerate() {
        builder
            .push(u32::try_from(item_no).ok()?, value.as_deref())
            .ok()?;
    }
    Some(builder.finish())
}

/// Stores compressed file evidence in node order while a result is being built.
#[derive(Debug, Default)]
pub(crate) struct PhysicalObservationStore {
    chunks: Vec<Option<Arc<PhysicalChunk>>>,
    present_count: usize,
}

impl PhysicalObservationStore {
    pub(crate) fn record(
        &mut self,
        node: u32,
        observation: &SpaceObservation,
    ) -> Result<(), BuildError> {
        let chunk_index = (node >> CHUNK_BITS) as usize;
        self.chunks.resize_with(chunk_index + 1, || None);
        let chunk =
            self.chunks[chunk_index].get_or_insert_with(|| Arc::new(PhysicalChunk::default()));
        Arc::make_mut(chunk).push((node & CHUNK_MASK) as u16, observation)?;
        self.present_count = self
            .present_count
            .checked_add(1)
            .ok_or(BuildError::CapacityExceeded)?;
        Ok(())
    }

    pub(crate) fn get(&self, node: u32) -> Option<PhysicalObservationRef<'_>> {
        let chunk = self.chunks.get((node >> CHUNK_BITS) as usize)?.as_ref()?;
        let record = chunk.record((node & CHUNK_MASK) as u16)?;
        Some(PhysicalObservationRef::Compact {
            record,
            arena: chunk.arena(),
        })
    }

    pub(crate) fn snapshot(&self, order: Arc<Vec<u32>>) -> Option<PhysicalObservations> {
        (self.present_count > 0).then(|| {
            PhysicalObservations::from_node_chunks(self.chunks.clone(), order, self.present_count)
        })
    }

    pub(crate) fn finish(&mut self, order: Arc<Vec<u32>>) -> Option<PhysicalObservations> {
        if self.present_count == 0 {
            return None;
        }
        let mut chunks = std::mem::take(&mut self.chunks);
        let present_count = std::mem::take(&mut self.present_count);
        for chunk in chunks.iter_mut().flatten() {
            if let Some(chunk) = Arc::get_mut(chunk) {
                chunk.clear_builder_index();
            }
        }
        Some(PhysicalObservations::from_node_chunks(
            chunks,
            order,
            present_count,
        ))
    }
}

#[cfg(test)]
pub(crate) struct PhysicalObservationsBuilder {
    records: Records,
    arena: Vec<u8>,
    volumes: HashMap<Vec<u8>, (u32, u32)>,
    present_count: usize,
}

#[cfg(test)]
impl PhysicalObservationsBuilder {
    fn new(item_count: usize, present_count: usize) -> Self {
        let dense_threshold = item_count.saturating_mul(10).div_ceil(11);
        let records = if present_count >= dense_threshold {
            Records::Dense(Vec::with_capacity(item_count))
        } else {
            Records::Sparse {
                item_numbers: Vec::with_capacity(present_count),
                records: Vec::with_capacity(present_count),
            }
        };
        Self {
            records,
            arena: Vec::new(),
            volumes: HashMap::new(),
            present_count,
        }
    }

    pub(crate) fn push(
        &mut self,
        item_no: u32,
        observation: Option<&SpaceObservation>,
    ) -> Result<(), BuildError> {
        let record = match observation {
            Some(observation) => encode(observation, &mut self.arena, &mut self.volumes)?,
            None => PhysicalRecord::default(),
        };
        match &mut self.records {
            Records::Dense(records) => records.push(record),
            Records::Sparse {
                item_numbers,
                records,
            } => {
                if observation.is_some() {
                    item_numbers.push(item_no);
                    records.push(record);
                }
            }
            Records::NodeChunks { .. } => return Err(BuildError::InvalidTree),
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> PhysicalObservations {
        PhysicalObservations {
            records: self.records,
            arena: self.arena,
            present_count: self.present_count,
        }
    }
}
