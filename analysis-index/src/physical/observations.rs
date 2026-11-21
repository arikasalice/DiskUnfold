//! Compact, immutable per-item physical observations.

use super::{FileIdentity, SharingEvidence, SpaceObservation};
use crate::BuildError;
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, SizeValue};

#[cfg(test)]
use std::sync::Arc;

const HAS_OBSERVATION: u32 = 1 << 0;
const HAS_ALLOCATION: u32 = 1 << 1;
const HAS_IDENTITY: u32 = 1 << 2;
const INLINE_IDENTITY: u32 = 1 << 3;
const HAS_LINK_COUNT: u32 = 1 << 4;
const SHARING_UNKNOWN: u32 = 1 << 5;

#[derive(Clone, Debug)]
pub(crate) struct PhysicalRecord {
    flags: u32,
    allocated: SizeValue,
    link_count: u64,
    identity: [u8; 16],
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
pub(crate) struct PhysicalObservationRef<'a> {
    record: &'a PhysicalRecord,
    arena: &'a [u8],
}

impl<'a> PhysicalObservationRef<'a> {
    pub(crate) fn allocated(self) -> Option<&'a SizeValue> {
        (self.record.flags & HAS_ALLOCATION != 0).then_some(&self.record.allocated)
    }

    pub(crate) fn identity(self) -> Option<FileIdentityRef<'a>> {
        let record = self.record;
        if record.flags & HAS_IDENTITY == 0 {
            return None;
        }
        let (volume, file) = if record.flags & INLINE_IDENTITY != 0 {
            (&record.identity[..8], &record.identity[8..])
        } else {
            let offset =
                usize::try_from(u64::from_le_bytes(record.identity[..8].try_into().ok()?)).ok()?;
            let volume_len_end = offset.checked_add(8)?;
            let file_len_end = volume_len_end.checked_add(8)?;
            let volume_len = usize::try_from(u64::from_le_bytes(
                self.arena.get(offset..volume_len_end)?.try_into().ok()?,
            ))
            .ok()?;
            let file_len = usize::try_from(u64::from_le_bytes(
                self.arena
                    .get(volume_len_end..file_len_end)?
                    .try_into()
                    .ok()?,
            ))
            .ok()?;
            let volume_end = file_len_end.checked_add(volume_len)?;
            let file_end = volume_end.checked_add(file_len)?;
            (
                self.arena.get(file_len_end..volume_end)?,
                self.arena.get(volume_end..file_end)?,
            )
        };
        Some(FileIdentityRef { volume, file })
    }

    pub(crate) fn link_count(self) -> Option<u64> {
        (self.record.flags & HAS_LINK_COUNT != 0).then_some(self.record.link_count)
    }

    pub(crate) fn sharing(self) -> SharingEvidence {
        if self.record.flags & SHARING_UNKNOWN != 0 {
            SharingEvidence::Unknown
        } else {
            SharingEvidence::None
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
    Dense(Vec<PhysicalRecord>),
    Sparse {
        item_numbers: Vec<u32>,
        records: Vec<PhysicalRecord>,
    },
}

#[derive(Debug)]
pub(crate) struct PhysicalObservations {
    records: Records,
    arena: Vec<u8>,
    present_count: usize,
}

impl PhysicalObservations {
    pub(crate) fn builder(item_count: usize, present_count: usize) -> PhysicalObservationsBuilder {
        PhysicalObservationsBuilder::new(item_count, present_count)
    }

    pub(crate) fn get(&self, item_no: u32) -> Option<PhysicalObservationRef<'_>> {
        let record = match &self.records {
            Records::Dense(records) => records.get(item_no as usize)?,
            Records::Sparse {
                item_numbers,
                records,
            } => records.get(item_numbers.binary_search(&item_no).ok()?)?,
        };
        (record.flags & HAS_OBSERVATION != 0).then_some(PhysicalObservationRef {
            record,
            arena: &self.arena,
        })
    }

    pub(crate) fn present_count(&self) -> usize {
        self.present_count
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

pub(crate) struct PhysicalObservationsBuilder {
    records: Records,
    arena: Vec<u8>,
    present_count: usize,
}

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
            present_count,
        }
    }

    pub(crate) fn push(
        &mut self,
        item_no: u32,
        observation: Option<&SpaceObservation>,
    ) -> Result<(), BuildError> {
        let record = match observation {
            Some(observation) => encode(observation, &mut self.arena)?,
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

fn encode(
    observation: &SpaceObservation,
    arena: &mut Vec<u8>,
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
        } else {
            let offset = u64::try_from(arena.len()).map_err(|_| BuildError::CapacityExceeded)?;
            let volume_len =
                u64::try_from(identity.volume.len()).map_err(|_| BuildError::CapacityExceeded)?;
            let file_len =
                u64::try_from(identity.file.len()).map_err(|_| BuildError::CapacityExceeded)?;
            record.identity[..8].copy_from_slice(&offset.to_le_bytes());
            arena.extend_from_slice(&volume_len.to_le_bytes());
            arena.extend_from_slice(&file_len.to_le_bytes());
            arena.extend_from_slice(&identity.volume);
            arena.extend_from_slice(&identity.file);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(identity: FileIdentity, sharing: SharingEvidence) -> SpaceObservation {
        SpaceObservation {
            allocated: Some(SizeValue {
                basis: SizeBasis::Allocated,
                evidence: SizeEvidence::OsReported,
                amount: SizeAmount::Exact {
                    bytes: 17_u64.into(),
                },
            }),
            identity: Some(identity),
            link_count: Some(2),
            sharing,
        }
    }

    #[test]
    fn compact_column_round_trips_inline_and_arena_identities() -> Result<(), String> {
        let inline = observation(
            FileIdentity {
                volume: (0..8).collect(),
                file: (8..16).collect(),
            },
            SharingEvidence::Unknown,
        );
        let arena = observation(
            FileIdentity {
                volume: vec![1, 2, 3],
                file: vec![4; 12],
            },
            SharingEvidence::None,
        );
        let empty_identity = observation(
            FileIdentity {
                volume: Vec::new(),
                file: Vec::new(),
            },
            SharingEvidence::Unknown,
        );
        let values = vec![
            None,
            Some(Arc::new(inline.clone())),
            None,
            Some(Arc::new(arena.clone())),
            Some(Arc::new(empty_identity.clone())),
        ];
        let compact = compact_test_observations(Some(&values))
            .ok_or_else(|| "failed to compact sparse observations".to_owned())?;
        assert!(matches!(&compact.records, Records::Sparse { .. }));
        for (item_no, expected) in values.iter().enumerate() {
            assert_eq!(
                compact
                    .get(u32::try_from(item_no).map_err(|error| format!("{error}"))?)
                    .map(PhysicalObservationRef::to_owned),
                expected.as_deref().cloned()
            );
        }

        let mut builder = PhysicalObservations::builder(2, 2);
        for item_no in 0..2 {
            builder
                .push(item_no, Some(&inline))
                .map_err(|error| format!("{error:?}"))?;
        }
        let dense = builder.finish();
        assert!(matches!(&dense.records, Records::Dense(_)));
        assert_eq!(
            dense.get(1).map(PhysicalObservationRef::to_owned),
            Some(inline)
        );
        Ok(())
    }
}
