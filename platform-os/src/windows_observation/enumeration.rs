//! Bounded legacy and pre-metadata filtered enumeration.
use super::*;
use du_core_contracts::scan_observation::{FilterCallbackPhase, TimedFilteredBatch};

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

fn correlated_stream_allocation() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

fn correlated_next_entry_offset(value: u32, minimum: usize) -> Result<Option<usize>, PortError> {
    if value == 0 {
        return Ok(None);
    }
    let Ok(next) = usize::try_from(value) else {
        return Err(correlated_stream_corrupt());
    };
    if next < minimum || !next.is_multiple_of(8) {
        return Err(correlated_stream_corrupt());
    }
    Ok(Some(next))
}

impl WindowsDirectory {
    fn fail_correlated<T>(&mut self, error: PortError) -> Result<T, PortError> {
        self.correlated_error = Some(error.clone());
        Err(error)
    }

    /// Retains only records decoded from one bounded OS buffer.
    fn fill_correlated_records(&mut self) -> Result<Option<bool>, PortError> {
        if !self.correlated_records.is_empty() {
            return Ok(Some(true));
        }
        if self.finished {
            return Ok(Some(false));
        }
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(BUFFER_SIZE / size_of::<u64>())
            .map_err(|_| correlated_stream_allocation())?;
        buffer.resize(BUFFER_SIZE / size_of::<u64>(), 0u64);
        let class = if self.restart {
            FileIdBothDirectoryRestartInfo
        } else {
            FileIdBothDirectoryInfo
        };
        self.restart = false;
        // SAFETY: buffer is aligned, writable and has the byte length supplied.
        let result = unsafe {
            GetFileInformationByHandleEx(
                raw(&self.handle),
                class,
                buffer.as_mut_ptr().cast(),
                BUFFER_SIZE as u32,
            )
        };
        if let Err(error) = result {
            let failure = win_error(error);
            if failure.os_code == Some(18) {
                self.finished = true;
                return Ok(Some(false));
            }
            return Err(failure);
        }

        let mut offset = 0usize;
        loop {
            let header = offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
            if offset
                .checked_add(size_of::<FILE_ID_BOTH_DIR_INFO>())
                .is_none_or(|end| end > BUFFER_SIZE)
            {
                return Err(correlated_stream_corrupt());
            }
            // SAFETY: the checked range covers the fixed record and buffer is aligned.
            let item = unsafe {
                std::ptr::read_unaligned(
                    buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset)
                        .cast::<FILE_ID_BOTH_DIR_INFO>(),
                )
            };
            let Ok(name_len) = usize::try_from(item.FileNameLength) else {
                return Err(correlated_stream_corrupt());
            };
            if name_len == 0
                || name_len % 2 != 0
                || offset
                    .checked_add(header)
                    .and_then(|value| value.checked_add(name_len))
                    .is_none_or(|end| end > BUFFER_SIZE)
            {
                return Err(correlated_stream_corrupt());
            }
            let mut name = Vec::new();
            name.try_reserve_exact(name_len)
                .map_err(|_| correlated_stream_allocation())?;
            for position in (0..name_len).step_by(2) {
                // SAFETY: the complete name range was bounds checked above.
                let unit = unsafe {
                    std::ptr::read_unaligned(
                        buffer
                            .as_ptr()
                            .cast::<u8>()
                            .add(offset + header + position)
                            .cast::<u16>(),
                    )
                };
                name.extend_from_slice(&unit.to_le_bytes());
            }
            if name != [b'.', 0] && name != [b'.', 0, b'.', 0] {
                self.correlated_records
                    .try_reserve(1)
                    .map_err(|_| correlated_stream_allocation())?;
                self.correlated_records.push_back(CorrelatedWindowsRecord {
                    name,
                    file_id: item.FileId as u64,
                    attributes: item.FileAttributes,
                    ea_size: item.EaSize,
                });
            }
            let Some(next) = correlated_next_entry_offset(item.NextEntryOffset, header + name_len)?
            else {
                break;
            };
            let Some(next_offset) = offset.checked_add(next) else {
                return Err(correlated_stream_corrupt());
            };
            offset = next_offset;
        }
        Ok((!self.correlated_records.is_empty()).then_some(true))
    }
}

impl DirectoryObservations for WindowsDirectory {
    fn begin_correlated_filtered(&mut self) -> Result<(), PortError> {
        if self.correlated || self.filtered.is_some() || self.finished || !self.pending.is_empty() {
            return Err(correlated_stream_conflict());
        }
        self.correlated = true;
        self.correlated_records.clear();
        self.correlated_next_child_id = Some(0);
        self.correlated_pending_observation = None;
        self.correlated_error = None;
        self.correlated_ended = false;
        Ok(())
    }

    fn next_correlated_event(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Result<CorrelatedDirectoryEvent, PortError> {
        if !self.correlated {
            return Err(correlated_stream_conflict());
        }
        if self.correlated_ended {
            return Err(correlated_stream_conflict());
        }
        if let Some(error) = &self.correlated_error {
            return Err(error.clone());
        }
        if let Some((child_id, entry)) = self.correlated_pending_observation.take() {
            return Ok(CorrelatedDirectoryEvent::Observation { child_id, entry });
        }
        match self.fill_correlated_records() {
            Ok(Some(true)) => {}
            Ok(Some(false)) => {
                let Some(child_count_exclusive) = self.correlated_next_child_id else {
                    return self.fail_correlated(correlated_stream_corrupt());
                };
                self.correlated_ended = true;
                return Ok(CorrelatedDirectoryEvent::End {
                    child_count_exclusive,
                });
            }
            Ok(None) => return Ok(CorrelatedDirectoryEvent::Progress),
            Err(error) => return self.fail_correlated(error),
        }

        let Some(record) = self.correlated_records.pop_front() else {
            return self.fail_correlated(correlated_stream_corrupt());
        };
        let Some(child_id) = self.correlated_next_child_id else {
            return self.fail_correlated(correlated_stream_corrupt());
        };
        let decision = include(&record.name);
        let mut callback_name = Vec::new();
        if callback_name.try_reserve_exact(record.name.len()).is_err() {
            return self.fail_correlated(correlated_stream_allocation());
        }
        callback_name.extend_from_slice(&record.name);
        self.correlated_next_child_id = child_id.checked_add(1);
        let callback = CorrelatedDirectoryEvent::FilterCallback {
            child_id,
            raw_name: callback_name,
            decision,
            phase: FilterCallbackPhase::BeforeMetadata,
        };
        if decision == FilterDecision::Admit {
            let entry = self.observe(
                record.name,
                record.file_id,
                record.attributes,
                record.ea_size,
            );
            self.correlated_pending_observation = Some((child_id, recall::timed(entry, clock)));
        }
        Ok(callback)
    }

    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(self
            .next_batch_timed(&|| None)?
            .into_iter()
            .map(|item| item.entry)
            .collect())
    }
    fn next_batch_timed(
        &mut self,
        clock: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Result<Vec<TimedObservedEntry>, PortError> {
        if self.correlated {
            return Err(correlated_stream_conflict());
        }
        if self.filtered == Some(true) {
            return Err(invalid());
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
        clock: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Result<TimedFilteredBatch, PortError> {
        if self.correlated {
            return Err(correlated_stream_conflict());
        }
        if self.filtered == Some(false) {
            return Err(invalid());
        }
        self.filtered = Some(true);
        if self.filter_failed {
            return Ok(TimedFilteredBatch::Undecidable);
        }
        if self.pending.is_empty() && !self.finished {
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

#[cfg(test)]
mod correlated_offset_tests {
    use super::*;

    #[test]
    fn next_entry_offset_obeys_native_alignment_and_record_bounds() {
        let minimum =
            std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName) + std::mem::size_of::<u16>();
        let aligned = (minimum + 7) & !7;
        let misaligned = if aligned - 1 >= minimum {
            aligned - 1
        } else {
            aligned + 1
        };
        let short_aligned = aligned - 8;

        assert!(misaligned >= minimum);
        assert!(!misaligned.is_multiple_of(8));
        assert!(short_aligned < minimum);
        assert!(matches!(
            correlated_next_entry_offset(aligned as u32, minimum),
            Ok(Some(value)) if value == aligned
        ));
        assert!(correlated_next_entry_offset(misaligned as u32, minimum).is_err());
        assert!(correlated_next_entry_offset(short_aligned as u32, minimum).is_err());
        assert!(matches!(correlated_next_entry_offset(0, minimum), Ok(None)));
    }
}
