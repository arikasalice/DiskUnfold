use du_scan_engine::watch_coalescer::{Batch, Gap, Key};
use std::collections::VecDeque;
use std::mem::size_of;

pub(super) const MAX_RECORDS: usize = 1024;
pub(super) const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Retains only gap metadata; no change paths or change hints.
#[derive(Clone, Debug)]
pub(super) struct Record {
    pub unit: [u8; 16],
    pub sequence: u64,
    pub generation: u64,
    pub end: u64,
    pub gaps: Vec<Gap>,
    pub replaced: Vec<u64>,
}
impl Record {
    pub fn from_batch(b: &Batch) -> Self {
        Self {
            unit: b.context.watch_ref,
            sequence: b.change_sequence,
            generation: b.context.generation,
            end: b.interval_end,
            gaps: b.gaps.clone(),
            replaced: b.replaced_gap_ids.clone(),
        }
    }
    pub fn bytes(&self) -> Option<usize> {
        let mut total = size_of::<Self>()
            .checked_add(self.gaps.len().checked_mul(size_of::<Gap>())?)?
            .checked_add(self.replaced.len().checked_mul(size_of::<u64>())?)?;
        for gap in &self.gaps {
            if let Some(path) = &gap.path {
                total = total.checked_add(
                    path.len()
                        .checked_mul(size_of::<<Key as IntoIterator>::Item>())?,
                )?;
                for part in path {
                    total = total.checked_add(part.len())?;
                }
            }
        }
        Some(total)
    }
}

#[derive(Default, Debug)]
pub(super) struct Window {
    pub records: VecDeque<Record>,
    pub bytes: usize,
}
impl Window {
    pub fn push(&mut self, record: Record) {
        let Some(bytes) = record.bytes().filter(|&n| n <= MAX_BYTES) else {
            return;
        };
        while self.records.len() >= MAX_RECORDS
            || self.bytes.checked_add(bytes).is_none_or(|n| n > MAX_BYTES)
        {
            self.pop();
        }
        self.bytes += bytes;
        self.records.push_back(record);
    }
    fn pop(&mut self) {
        if let Some(old) = self.records.pop_front() {
            self.bytes -= old.bytes().unwrap_or(self.bytes);
        }
    }
    pub fn remove_unit_through(&mut self, unit: [u8; 16], cut: u64) {
        self.records.retain(|r| r.unit != unit || r.sequence > cut);
        self.bytes = self.records.iter().filter_map(Record::bytes).sum();
    }
}
