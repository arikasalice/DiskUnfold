use super::buffer::Buffer;
use super::model::{Batch, Config, Context, Cursor, Error, Gap, GapReason, Key, Signal, Trust};
use std::collections::BTreeSet;

/// One pure, deterministic coalescing stream for a verified watch generation.
///
/// Methods require exclusive access. Errors leave all buffered evidence, gap
/// identifiers, timestamps, and watch sequence unchanged. The caller must
/// deliver every returned batch in order and must not call an OS adapter here.
pub struct Coalescer {
    context: Context,
    config: Config,
    buffer: Buffer,
    open_gaps: Vec<Gap>,
    next_gap: u64,
    sequence: u64,
    last_time: Option<u64>,
    last_normal: Option<u64>,
}
impl Coalescer {
    /// Creates a stream with the last allocated per-unit watch sequence and
    /// currently open gap evidence. Context and open gaps are supplied by the
    /// session registry; no identifier or timestamp is generated here.
    pub fn new(
        context: Context,
        config: Config,
        cursor: Cursor,
        open_gaps: Vec<Gap>,
    ) -> Result<Self, Error> {
        if context.generation == 0
            || context.volume.is_empty()
            || config.window_ms == 0
            || config.promote_at < 2
            || config.queue_limit == 0
            || config.gap_limit == 0
        {
            return Err(Error::InvalidConfig);
        }
        if open_gaps.len() > config.gap_limit {
            return Err(Error::GapBudget {
                required: open_gaps.len(),
                available: config.gap_limit,
            });
        }
        let mut ids = BTreeSet::new();
        if open_gaps
            .iter()
            .any(|g| g.id == 0 || g.end.is_some() || !ids.insert(g.id))
        {
            return Err(Error::InvalidConfig);
        }
        if ids.last().is_some_and(|id| *id > cursor.last_gap_id) {
            return Err(Error::InvalidConfig);
        }
        let next_gap = cursor
            .last_gap_id
            .checked_add(1)
            .ok_or(Error::GapIdExhausted)?;
        Ok(Self {
            context,
            config,
            buffer: Buffer::default(),
            open_gaps,
            next_gap,
            sequence: cursor.sequence,
            last_time: None,
            last_normal: None,
        })
    }
    /// Last allocated watch sequence, including status reservations.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    /// Last allocated per-unit gap identifier, for session registry handoff.
    pub fn last_gap_id(&self) -> u64 {
        self.next_gap - 1
    }
    /// Number of currently buffered, coalesced directory ranges.
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }
    /// Changes only the explicit gap budget. Existing evidence is never dropped.
    pub fn set_gap_limit(&mut self, limit: usize) -> Result<(), Error> {
        if limit == 0 {
            return Err(Error::InvalidConfig);
        }
        if limit < self.open_gaps.len() {
            return Err(Error::GapBudget {
                required: self.open_gaps.len(),
                available: limit,
            });
        }
        self.config.gap_limit = limit;
        Ok(())
    }
    /// Reserves the next number for a status event in the same per-unit stream.
    /// The caller must publish that event before invoking another operation.
    pub fn reserve_status_sequence(&mut self) -> Result<u64, Error> {
        self.next_sequence()
    }
    /// Accepts a prevalidated signal. A stale generation is ignored unchanged.
    /// Keep the signal until this method succeeds so an error can be retried.
    pub fn signal(&mut self, generation: u64, signal: &Signal) -> Result<Vec<Batch>, Error> {
        if generation != self.context.generation {
            return Ok(Vec::new());
        }
        self.check_time(signal.observed_at)?;
        let output = if self.buffer.due(self.config, signal.observed_at) {
            self.signal_after_window(signal)?
        } else {
            self.signal_in_window(signal)?
        };
        self.last_time = Some(signal.observed_at);
        self.last_normal = Some(signal.observed_at);
        Ok(output)
    }
    fn signal_in_window(&mut self, signal: &Signal) -> Result<Vec<Batch>, Error> {
        let undo = self.buffer.push(signal, self.config);
        if self.buffer.len() < self.config.queue_limit {
            return Ok(Vec::new());
        }
        if let Err(error) = self.preflight(1, true) {
            self.buffer.rollback(undo);
            return Err(error);
        }
        self.overflow(signal.observed_at)
    }
    fn signal_after_window(&mut self, signal: &Signal) -> Result<Vec<Batch>, Error> {
        // Only the new signal is staged: the old window is never copied.
        let mut next = Buffer::default();
        next.push(signal, self.config);
        let overflows = next.len() >= self.config.queue_limit;
        let batches = self.buffer.len().div_ceil(32) + usize::from(overflows);
        self.preflight(batches, overflows)?;
        let mut output = self.flush_buffer(signal.observed_at)?;
        self.buffer = next;
        if overflows {
            output.extend(self.overflow(signal.observed_at)?);
        }
        Ok(output)
    }
    fn overflow(&mut self, at: u64) -> Result<Vec<Batch>, Error> {
        let first = self.buffer.first().unwrap_or(at);
        let gap = self.new_gap(GapReason::Overflow, None, Some(first), Some(at))?;
        self.buffer.take();
        self.make_batches(Vec::new(), vec![gap], first, at)
    }
    /// Delivers a loss notice, flushing the current window with gap evidence.
    /// `path = None` marks the entire watch unit; a supplied path covers its subtree.
    pub fn signal_lost(
        &mut self,
        generation: u64,
        path: Option<&Key>,
        at: u64,
    ) -> Result<Vec<Batch>, Error> {
        if generation != self.context.generation {
            return Ok(Vec::new());
        }
        self.check_time(at)?;
        let due = self.buffer.due(self.config, at);
        let batches = if due {
            self.buffer.len().div_ceil(32) + 1
        } else {
            self.buffer.len().div_ceil(32).max(1)
        };
        self.preflight(batches, true)?;
        let mut output = if due {
            self.flush_buffer(at)?
        } else {
            Vec::new()
        };
        let gap = self.new_gap(
            GapReason::SignalLost,
            path.cloned(),
            self.last_normal,
            Some(at),
        )?;
        let start = self.buffer.opened.unwrap_or(at);
        let changes = self.buffer.take();
        output.extend(self.make_batches(changes, vec![gap], start, at)?);
        self.last_time = Some(at);
        Ok(output)
    }
    /// Flushes only after the 1,000 ms default window expires. Stale generations
    /// are ignored; a caller may use `flush` to close a window on stop.
    pub fn tick(&mut self, generation: u64, at: u64) -> Result<Vec<Batch>, Error> {
        if generation != self.context.generation {
            return Ok(Vec::new());
        }
        self.check_time(at)?;
        let output = if self.buffer.due(self.config, at) {
            self.checked_flush(at)?
        } else {
            Vec::new()
        };
        self.last_time = Some(at);
        Ok(output)
    }
    /// Closes a pending window, returning no batch for an empty window.
    pub fn flush(&mut self, generation: u64, at: u64) -> Result<Vec<Batch>, Error> {
        if generation != self.context.generation {
            return Ok(Vec::new());
        }
        self.check_time(at)?;
        let output = self.checked_flush(at)?;
        self.last_time = Some(at);
        Ok(output)
    }
    fn check_time(&self, at: u64) -> Result<(), Error> {
        if self.last_time.is_some_and(|last| at < last) {
            return Err(Error::ClockRegressed);
        }
        Ok(())
    }
    // Preflight every fallible allocation of identifiers before taking a buffer
    // or emitting a batch. Extra gaps include loss and overflow markers.
    fn preflight(&self, batches: usize, extra_gap: bool) -> Result<(), Error> {
        if extra_gap {
            self.next_gap.checked_add(1).ok_or(Error::GapIdExhausted)?;
        }
        let required = self.open_gaps.len() + usize::from(extra_gap);
        if required > self.config.gap_limit {
            return Err(Error::GapBudget {
                required,
                available: self.config.gap_limit,
            });
        }
        let count = u64::try_from(batches).map_err(|_| Error::SequenceExhausted)?;
        self.sequence
            .checked_add(count)
            .ok_or(Error::SequenceExhausted)?;
        Ok(())
    }
    fn checked_flush(&mut self, at: u64) -> Result<Vec<Batch>, Error> {
        self.preflight(self.buffer.len().div_ceil(32), false)?;
        self.flush_buffer(at)
    }
    fn flush_buffer(&mut self, at: u64) -> Result<Vec<Batch>, Error> {
        if self.buffer.is_empty() {
            return Ok(Vec::new());
        }
        let start = self.buffer.opened.unwrap_or(at);
        let changes = self.buffer.take();
        self.make_batches(changes, Vec::new(), start, at)
    }
    fn new_gap(
        &mut self,
        reason: GapReason,
        path: Option<Key>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<Gap, Error> {
        let id = self.next_gap;
        self.next_gap = id.checked_add(1).ok_or(Error::GapIdExhausted)?;
        Ok(Gap {
            id,
            reason,
            path,
            start,
            end,
        })
    }
    fn next_sequence(&mut self) -> Result<u64, Error> {
        let next = self
            .sequence
            .checked_add(1)
            .ok_or(Error::SequenceExhausted)?;
        self.sequence = next;
        Ok(next)
    }
    fn make_batches(
        &mut self,
        changes: Vec<super::model::Change>,
        mut gaps: Vec<Gap>,
        start: u64,
        end: u64,
    ) -> Result<Vec<Batch>, Error> {
        gaps.extend(self.open_gaps.iter().cloned());
        if gaps.len() > self.config.gap_limit {
            return Err(Error::GapBudget {
                required: gaps.len(),
                available: self.config.gap_limit,
            });
        }
        let trust = if gaps.is_empty() {
            Trust::Normal
        } else {
            Trust::Gap
        };
        let mut batches = Vec::new();
        if changes.is_empty() {
            if gaps.is_empty() {
                return Ok(batches);
            }
            let sequence = self.next_sequence()?;
            batches.push(Batch {
                context: self.context.clone(),
                sequence,
                interval_start: start,
                interval_end: end,
                changes,
                gaps,
                trust,
            });
            return Ok(batches);
        }
        for (index, chunk) in changes.chunks(32).enumerate() {
            let sequence = self.next_sequence()?;
            batches.push(Batch {
                context: self.context.clone(),
                sequence,
                interval_start: start,
                interval_end: end,
                changes: chunk.to_vec(),
                gaps: if index == 0 { gaps.clone() } else { Vec::new() },
                trust,
            });
        }
        Ok(batches)
    }
}
