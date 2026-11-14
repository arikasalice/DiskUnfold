//! Bounded legacy and pre-metadata filtered enumeration over bulk records.
use super::{MacDirectory, bulk, errors};
use du_core_contracts::{
    Timestamp,
    ports::PortError,
    scan_observation::{
        DirectoryObservations, FilterDecision, FilteredBatch, MetadataObservation, ObservedEntry,
        ObservedKind, SpaceIdentity, TimedFilteredBatch, TimedObservedEntry,
    },
};
use std::os::fd::AsFd;

/// Largest number of entries one batch returns.
const BATCH_SIZE: usize = 256;

impl MacDirectory {
    /// Reads one bulk buffer. Names are decided before any entry is recorded;
    /// excluded records are dropped and no item is ever opened. An error ends
    /// the directory, and so does one malformed record (see [`bulk::parse`]).
    ///
    /// The bulk buffer is allocated on the first read of this handle, reused by
    /// later reads and released when the directory ends.
    fn fill(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<(), PortError> {
        if self.buffer.is_empty() {
            self.buffer = vec![0u8; bulk::BUFFER_BYTES];
        }
        let records = match bulk::read(self.fd.as_fd(), &mut self.buffer) {
            Ok(records) => records,
            Err(error) => {
                self.finish();
                return Err(error);
            }
        };
        if records.is_empty() {
            self.finish();
            return Ok(());
        }
        for record in records {
            match include(&record.name) {
                FilterDecision::Admit => {
                    let entry = self.observe(record);
                    self.pending.push_back(timed(entry, clock));
                }
                FilterDecision::Exclude => {}
                FilterDecision::Undecidable => {
                    self.pending.clear();
                    self.filter_failed = true;
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self) {
        self.finished = true;
        self.buffer = Vec::new();
    }

    /// The enumerated and metadata views come from the same record, so they
    /// always agree; replacement is detected when a child is opened. For a
    /// mount point the record describes the covered directory of this volume.
    pub(super) fn observe(&self, record: bulk::Record) -> ObservedEntry {
        let kind = record.kind();
        let inode = record.file_id.unwrap_or(0);
        let metadata = self.metadata(&record, &kind, inode);
        ObservedEntry {
            name: record.name,
            enumerated_inode: inode,
            enumerated_kind: kind,
            metadata,
        }
    }

    /// Metadata of one record.
    ///
    /// `size` is the logical length of the entry itself, as `st_size`
    /// reports it: the data fork of a file, the target length of a symbolic
    /// link and the directory length of a directory. `allocated` is reported
    /// for files only and counts every fork, as `st_blocks` and `du` do.
    ///
    /// A mount point, or an entry on another device, follows the rule for
    /// another volume: identity and kind only.
    fn metadata(
        &self,
        record: &bulk::Record,
        kind: &ObservedKind,
        inode: u64,
    ) -> Result<MetadataObservation, PortError> {
        if let Some(code) = record.error.filter(|&code| code != 0) {
            return Err(errors::errno(code.cast_signed()));
        }
        let device = record.device.ok_or_else(errors::unavailable)?;
        if device != self.device || *kind == ObservedKind::MountPoint {
            return Ok(other_volume(kind.clone(), device, inode));
        }
        let file = *kind == ObservedKind::File;
        Ok(MetadataObservation {
            kind: kind.clone(),
            device,
            inode,
            size: record
                .length()
                .and_then(|bytes| u64::try_from(bytes).ok())
                .ok_or_else(errors::unavailable),
            modified_at: record
                .modified
                .and_then(|(seconds, nanoseconds)| bulk::timestamp(seconds, nanoseconds)),
            native_modified_at: None,
            allocated: record
                .allocated
                .filter(|_| file)
                .and_then(|bytes| u64::try_from(bytes).ok()),
            link_count: record
                .link_count
                .filter(|&count| file && count > 0)
                .map(u64::from),
            space_identity: (file && inode != 0).then(|| SpaceIdentity {
                volume: self.volume_id.to_vec(),
                file: inode.to_le_bytes().to_vec(),
            }),
        })
    }
}

/// An entry of another volume, or a mount point, with its identity and kind
/// only, as `MetadataObservation` requires. A mount point carries the device
/// and inode of the directory it covers, which is what the record reports.
pub(super) fn other_volume(kind: ObservedKind, device: u64, inode: u64) -> MetadataObservation {
    MetadataObservation {
        kind,
        device,
        inode,
        size: Err(errors::unavailable()),
        modified_at: None,
        native_modified_at: None,
        allocated: None,
        link_count: None,
        space_identity: None,
    }
}

/// macOS dataless state has no placeholder variant in the port, so none is claimed.
fn timed(entry: ObservedEntry, clock: &dyn Fn() -> Option<Timestamp>) -> TimedObservedEntry {
    let observed_at = if entry.metadata.is_ok() {
        clock()
    } else {
        None
    };
    TimedObservedEntry {
        entry,
        observed_at,
        placeholder_observation: None,
    }
}

impl DirectoryObservations for MacDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(self
            .next_batch_timed(&|| None)?
            .into_iter()
            .map(|item| item.entry)
            .collect())
    }

    fn next_batch_timed(
        &mut self,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<Vec<TimedObservedEntry>, PortError> {
        if self.filtered == Some(true) {
            return Err(errors::invalid());
        }
        self.filtered = Some(false);
        while self.pending.is_empty() && !self.finished {
            self.fill(&mut |_| FilterDecision::Admit, clock)?;
        }
        let count = self.pending.len().min(BATCH_SIZE);
        Ok(self.pending.drain(..count).collect())
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        Ok(match self.next_filtered_timed(include, &|| None)? {
            TimedFilteredBatch::Entries(items) => {
                FilteredBatch::Entries(items.into_iter().map(|item| item.entry).collect())
            }
            TimedFilteredBatch::Progress => FilteredBatch::Progress,
            TimedFilteredBatch::End => FilteredBatch::End,
            TimedFilteredBatch::Undecidable => FilteredBatch::Undecidable,
        })
    }

    fn next_filtered_timed(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<TimedFilteredBatch, PortError> {
        if self.filtered == Some(false) {
            return Err(errors::invalid());
        }
        self.filtered = Some(true);
        if !self.filter_failed && self.pending.is_empty() && !self.finished {
            self.fill(include, clock)?;
        }
        if self.filter_failed {
            return Ok(TimedFilteredBatch::Undecidable);
        }
        let count = self.pending.len().min(BATCH_SIZE);
        if count > 0 {
            return Ok(TimedFilteredBatch::Entries(
                self.pending.drain(..count).collect(),
            ));
        }
        if self.finished {
            Ok(TimedFilteredBatch::End)
        } else {
            Ok(TimedFilteredBatch::Progress)
        }
    }
}
