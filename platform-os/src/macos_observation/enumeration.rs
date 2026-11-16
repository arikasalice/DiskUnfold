//! Bounded legacy and pre-metadata filtered enumeration over bulk records.
use super::{MacDirectory, bulk, errors};
use du_core_contracts::{
    Timestamp,
    ports::{PortError, PortErrorKind},
    scan_observation::{
        CorrelatedDirectoryEvent, DirectoryObservations, FilterCallbackPhase, FilterDecision,
        FilteredBatch, MetadataObservation, ObservedEntry, ObservedKind, SpaceIdentity,
        TimedFilteredBatch, TimedObservedEntry,
    },
};
use std::os::fd::AsFd;

/// Largest number of entries one batch returns.
const BATCH_SIZE: usize = 256;

fn correlated_stream_conflict() -> PortError {
    PortError {
        kind: PortErrorKind::Busy,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

fn correlated_stream_corrupt() -> PortError {
    PortError {
        kind: PortErrorKind::Corrupt,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

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

impl MacDirectory {
    /// Retains at most the records parsed from one fixed-size bulk reply.
    fn fill_correlated_records(&mut self) -> Result<bool, PortError> {
        if !self.correlated_records.is_empty() {
            return Ok(true);
        }
        if self.finished {
            return Ok(false);
        }
        if self.buffer.is_empty() {
            let mut buffer = Vec::new();
            if buffer.try_reserve_exact(bulk::BUFFER_BYTES).is_err() {
                let error = bulk::allocation_error();
                self.finish();
                self.correlated_stream_error = Some(error.clone());
                return Err(error);
            }
            buffer.resize(bulk::BUFFER_BYTES, 0);
            self.buffer = buffer;
        }
        match bulk::read_correlated(self.fd.as_fd(), &mut self.buffer) {
            Ok(records) if records.is_empty() => {
                self.finish();
                Ok(false)
            }
            Ok(records) => {
                self.correlated_records = records.into();
                Ok(true)
            }
            Err(error) => {
                self.finish();
                self.correlated_stream_error = Some(error.clone());
                Err(error)
            }
        }
    }

    fn correlated_event(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<CorrelatedDirectoryEvent, PortError> {
        if !self.correlated {
            return Err(correlated_stream_conflict());
        }
        if self.correlated_ended {
            return Err(correlated_stream_conflict());
        }
        if let Some((child_id, entry)) = self.correlated_pending_observation.take() {
            return Ok(CorrelatedDirectoryEvent::Observation { child_id, entry });
        }
        if let Some(error) = &self.correlated_stream_error {
            return Err(error.clone());
        }
        if !self.fill_correlated_records()? {
            let Some(child_count_exclusive) = self.correlated_next_child_id else {
                let error = correlated_stream_corrupt();
                self.correlated_stream_error = Some(error.clone());
                return Err(error);
            };
            self.correlated_ended = true;
            return Ok(CorrelatedDirectoryEvent::End {
                child_count_exclusive,
            });
        }
        let record = match self.correlated_records.pop_front() {
            Some(record) => record,
            None => {
                let error = correlated_stream_corrupt();
                self.correlated_stream_error = Some(error.clone());
                return Err(error);
            }
        };
        let child_id = match self.correlated_next_child_id {
            Some(child_id) => child_id,
            None => {
                let error = correlated_stream_corrupt();
                self.correlated_stream_error = Some(error.clone());
                return Err(error);
            }
        };
        let raw_name = match bulk::try_copy_bytes(&record.name) {
            Ok(name) => name,
            Err(error) => {
                self.correlated_stream_error = Some(error.clone());
                return Err(error);
            }
        };
        self.correlated_next_child_id = child_id.checked_add(1);
        if self.correlated_next_child_id.is_none() {
            self.correlated_stream_error = Some(correlated_stream_corrupt());
        }
        let decision = include(&record.name);
        if decision == FilterDecision::Admit {
            let entry = self.observe(record);
            let observed_at = if entry.metadata.is_ok() {
                clock()
            } else {
                None
            };
            self.correlated_pending_observation = Some((
                child_id,
                TimedObservedEntry {
                    entry,
                    observed_at,
                    placeholder_observation: None,
                },
            ));
        }
        Ok(CorrelatedDirectoryEvent::FilterCallback {
            child_id,
            raw_name,
            decision,
            // getattrlistbulk has already returned this child's metadata record.
            phase: FilterCallbackPhase::AfterMetadata,
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
        if self.correlated {
            return Err(correlated_stream_conflict());
        }
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
        if self.correlated {
            return Err(correlated_stream_conflict());
        }
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

    fn begin_correlated_filtered(&mut self) -> Result<(), PortError> {
        if self.correlated || self.filtered.is_some() || self.finished {
            return Err(correlated_stream_conflict());
        }
        self.correlated = true;
        self.correlated_records.clear();
        self.correlated_next_child_id = Some(0);
        self.correlated_pending_observation = None;
        self.correlated_stream_error = None;
        self.correlated_ended = false;
        Ok(())
    }

    fn next_correlated_event(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<CorrelatedDirectoryEvent, PortError> {
        self.correlated_event(include, clock)
    }
}
