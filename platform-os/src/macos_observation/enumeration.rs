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
    /// the directory.
    fn fill(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<(), PortError> {
        let records = bulk::read(self.fd.as_fd()).inspect_err(|_| self.finished = true)?;
        if records.is_empty() {
            self.finished = true;
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

    /// The enumerated and metadata views come from the same record, so they
    /// always agree; replacement is detected when a child is opened.
    fn observe(&self, record: bulk::Record) -> ObservedEntry {
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
        if device != self.device {
            return Err(errors::invalid());
        }
        let file = *kind == ObservedKind::File;
        Ok(MetadataObservation {
            kind: kind.clone(),
            device,
            inode,
            size: record
                .data_length
                .and_then(|bytes| u64::try_from(bytes).ok())
                .ok_or_else(errors::unavailable),
            modified_at: record
                .modified
                .and_then(|(seconds, nanoseconds)| bulk::timestamp(seconds, nanoseconds)),
            native_modified_at: None,
            allocated: record
                .data_allocated
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
