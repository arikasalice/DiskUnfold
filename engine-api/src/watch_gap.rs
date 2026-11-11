//! F33's session-only IF-10/IF-11 consumer, before composition with IF-17.
//!
//! Calls are serialized by the scope owner. The supplied callback is the
//! synchronous IF-17 commit boundary: it must stage one normalized update and
//! publish its ledger state only if the complete update succeeds. Returning an
//! error means no part of that update was committed. The consumer advances its
//! cursor only after this boundary succeeds.
//!
//! A replacement scope revision uses a new `Ledger`; release the old instance
//! when its registry scope is released. These are in-process implementation
//! types, not a serialized wire contract.

pub mod batch;
mod gaps;
pub mod recovery;
mod replay;

use du_scan_engine::watch_coalescer::{Batch, Gap};
use du_scan_engine::watch_lifecycle::{Definition, Snapshot, State};
use replay::Window;
use std::collections::BTreeMap;

/// An ephemeral request to the future atomic freshness-ledger adapter.
pub enum Update<'a> {
    /// Apply one change and its gap replacement as one transaction.
    Batch {
        batch: &'a Batch,
        previous_gaps: &'a BTreeMap<u64, Gap>,
        gaps: &'a BTreeMap<u64, Gap>,
    },
    /// The volume mismatched; discard the batch's change ranges.
    VolumeReplaced {
        scope: [u8; 16],
        watch_ref: [u8; 16],
    },
    /// Replace only this unit's producer gaps and add the consumer resync gap.
    Snapshot {
        snapshot: &'a Snapshot,
        gaps: &'a BTreeMap<u64, Gap>,
        /// True when continuity was lost or the snapshot skips unprocessed batches.
        resync_required: bool,
        /// Last known batch end, independent of whether resynchronization is required.
        resync_start: Option<u64>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    Duplicate,
    ReleasedScope,
    Resync,
    VolumeReplaced,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    UnknownUnit,
    DuplicateUnit,
    WrongScope,
    InvalidSnapshot,
    Apply(E),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotCut {
    cut: u64,
    observed_at: u64,
    sequence: u64,
    digest: [u8; 32],
}

/// Consumer state is separate from the committed freshness ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub definition: Definition,
    pub generation: u64,
    pub last_change_sequence: u64,
    pub last_batch_end: Option<u64>,
    pub state: Option<State>,
    pub gaps: BTreeMap<u64, Gap>,
    /// The entire unit must remain unverified until successfully resynchronized.
    pub needs_resync: bool,
    pub volume_unverifiable: bool,
    snapshot: Option<SnapshotCut>,
    // Last received batch end at or before the applied snapshot cut. Replay
    // records through that cut are discarded, but later snapshots still need
    // this boundary to describe the consumer resynchronization gap.
    snapshot_batch_end: Option<u64>,
}

/// Owns one scope revision's watch consumers and bounded replay window.
pub struct Ledger {
    scope: [u8; 16],
    released: bool,
    units: BTreeMap<[u8; 16], Unit>,
    window: Window,
}
impl Ledger {
    pub fn new(scope: [u8; 16]) -> Self {
        Self {
            scope,
            released: false,
            units: BTreeMap::new(),
            window: Window::default(),
        }
    }
    pub fn register<E>(&mut self, definition: Definition) -> Result<(), Error<E>> {
        if self.released || definition.scope_revision != self.scope {
            return Err(Error::WrongScope);
        }
        if self.units.contains_key(&definition.watch_ref) {
            return Err(Error::DuplicateUnit);
        }
        self.units.insert(
            definition.watch_ref,
            Unit {
                definition,
                generation: 0,
                last_change_sequence: 0,
                last_batch_end: None,
                state: None,
                gaps: BTreeMap::new(),
                needs_resync: false,
                volume_unverifiable: false,
                snapshot: None,
                snapshot_batch_end: None,
            },
        );
        Ok(())
    }
    pub fn unit(&self, id: &[u8; 16]) -> Option<&Unit> {
        self.units.get(id)
    }
    /// Remove only a registry-released unit; its already committed freshness remains external.
    pub fn release_unit(&mut self, id: [u8; 16]) {
        self.units.remove(&id);
        self.window.remove_unit_through(id, u64::MAX);
    }
    /// Release the scope's consumer memory, without altering committed freshness.
    pub fn release(&mut self) {
        self.released = true;
        self.units.clear();
        self.window = Window::default();
    }
    pub fn require_resync(&mut self) {
        if !self.released {
            for unit in self.units.values_mut() {
                unit.needs_resync = true;
            }
        }
    }
    /// Consume a batch only after an initial snapshot and its IF-17 transaction succeed.
    ///
    /// A newly registered unit has no trusted change-sequence baseline. Until
    /// its snapshot callback succeeds, batches request resynchronization and
    /// neither the sink nor the consumer cursor is advanced. The callback is
    /// the transaction's linearization point and must roll back all staged
    /// ledger changes before returning an error.
    pub fn apply<E>(
        &mut self,
        batch: &Batch,
        sink: &mut impl FnMut(Update<'_>) -> Result<(), E>,
    ) -> Result<Outcome, Error<E>> {
        if self.released || batch.context.scope_revision != self.scope {
            return Ok(Outcome::ReleasedScope);
        }
        let unit = self
            .units
            .get_mut(&batch.context.watch_ref)
            .ok_or(Error::UnknownUnit)?;
        if unit.snapshot.is_none() {
            unit.needs_resync = true;
            return Ok(Outcome::Resync);
        }
        if batch.change_sequence <= unit.last_change_sequence {
            return Ok(Outcome::Duplicate);
        }
        if unit.needs_resync
            || unit.last_change_sequence.checked_add(1) != Some(batch.change_sequence)
            || batch.context.generation == 0
            || batch.context.generation < unit.generation
            || batch.interval_start > batch.interval_end
        {
            unit.needs_resync = true;
            return Ok(Outcome::Resync);
        }
        let mismatch = batch.context.volume != unit.definition.volume;
        let next = if mismatch {
            unit.gaps.clone()
        } else if let Some(g) = gaps::apply(&unit.gaps, &batch.gaps, &batch.replaced_gap_ids) {
            g
        } else {
            unit.needs_resync = true;
            return Ok(Outcome::Resync);
        };
        let update = if mismatch {
            Update::VolumeReplaced {
                scope: self.scope,
                watch_ref: batch.context.watch_ref,
            }
        } else {
            Update::Batch {
                batch,
                previous_gaps: &unit.gaps,
                gaps: &next,
            }
        };
        if let Err(e) = sink(update) {
            unit.needs_resync = true;
            return Err(Error::Apply(e));
        }
        unit.gaps = next;
        unit.volume_unverifiable |= mismatch;
        if mismatch || batch.context.generation != unit.generation {
            unit.state = None;
        }
        unit.generation = batch.context.generation;
        unit.last_change_sequence = batch.change_sequence;
        unit.last_batch_end = Some(batch.interval_end);
        if !mismatch {
            self.window.push_batch(batch);
        }
        Ok(if mismatch {
            Outcome::VolumeReplaced
        } else {
            Outcome::Applied
        })
    }
    /// Apply a snapshot and replay retained later gap metadata, never raw changes.
    pub fn apply_snapshot<E>(
        &mut self,
        snapshot: &Snapshot,
        sink: &mut impl FnMut(Update<'_>) -> Result<(), E>,
    ) -> Result<Outcome, Error<E>> {
        if self.released || snapshot.definition.scope_revision != self.scope {
            return Ok(Outcome::ReleasedScope);
        }
        let unit = self
            .units
            .get_mut(&snapshot.definition.watch_ref)
            .ok_or(Error::UnknownUnit)?;
        if snapshot.definition != unit.definition
            || (snapshot.generation == 0 && snapshot.last_change_sequence != 0)
        {
            unit.needs_resync = true;
            return Ok(Outcome::Resync);
        }
        if snapshot
            .last_observed
            .is_some_and(|at| at > snapshot.observed_at)
            || snapshot
                .gaps
                .iter()
                .any(|gap| !gaps::valid_at(gap, snapshot.observed_at))
        {
            unit.needs_resync = true;
            return Err(Error::InvalidSnapshot);
        }
        let cut = snapshot_cut(snapshot);
        if let Some(outcome) = snapshot_order(unit, &cut) {
            return Ok(outcome);
        }
        let mut next = BTreeMap::new();
        for gap in &snapshot.gaps {
            if next.insert(gap.id, gap.clone()).is_some() {
                unit.needs_resync = true;
                return Err(Error::InvalidSnapshot);
            }
        }
        let Some((next, generation)) = replay_snapshot(unit, &self.window, snapshot, next) else {
            unit.needs_resync = true;
            return Ok(Outcome::Resync);
        };
        let prior = self
            .window
            .records
            .iter()
            .rfind(|r| r.unit == snapshot.definition.watch_ref && r.sequence <= cut.cut)
            .map(|r| r.end)
            .or_else(|| {
                (cut.cut >= unit.last_change_sequence)
                    .then_some(unit.last_batch_end)
                    .flatten()
            })
            .or(unit.snapshot_batch_end);
        if prior.is_some_and(|t| t > snapshot.observed_at) {
            unit.needs_resync = true;
            return Ok(Outcome::Resync);
        }
        if let Err(e) = sink(Update::Snapshot {
            snapshot,
            gaps: &next,
            resync_required: unit.needs_resync || cut.cut > unit.last_change_sequence,
            resync_start: prior,
        }) {
            unit.needs_resync = true;
            return Err(Error::Apply(e));
        }
        unit.gaps = next;
        unit.generation = generation;
        unit.state = if cut.cut < unit.last_change_sequence {
            None
        } else {
            Some(snapshot.state)
        };
        unit.last_change_sequence = unit.last_change_sequence.max(cut.cut);
        unit.snapshot = Some(cut);
        unit.snapshot_batch_end = prior;
        unit.needs_resync = false;
        self.window
            .remove_unit_through(snapshot.definition.watch_ref, snapshot.last_change_sequence);
        Ok(Outcome::Applied)
    }
}

fn snapshot_cut(s: &Snapshot) -> SnapshotCut {
    // Excludes last_observed: buffered signals may update it without a published sequence.
    // Gap evidence is keyed by ID, so its enumeration order is not content.
    let mut ordered_gaps: Vec<_> = s.gaps.iter().collect();
    ordered_gaps.sort_by_key(|gap| gap.id);
    let state = format!("{:?}|{}|{:?}", s.state, s.generation, ordered_gaps);
    SnapshotCut {
        cut: s.last_change_sequence,
        observed_at: s.observed_at,
        sequence: s.sequence,
        digest: *blake3::hash(state.as_bytes()).as_bytes(),
    }
}
fn snapshot_order(unit: &mut Unit, cut: &SnapshotCut) -> Option<Outcome> {
    let old = unit.snapshot.as_ref()?;
    if old == cut {
        return Some(if unit.needs_resync {
            Outcome::Resync
        } else {
            Outcome::Duplicate
        });
    }
    if cut.cut < old.cut
        || cut.observed_at < old.observed_at
        || cut.sequence < old.sequence
        || (cut.sequence == old.sequence && (cut.cut != old.cut || cut.digest != old.digest))
    {
        unit.needs_resync = true;
        return Some(Outcome::Resync);
    }
    None
}
fn replay_snapshot(
    unit: &Unit,
    window: &Window,
    s: &Snapshot,
    mut gaps: BTreeMap<u64, Gap>,
) -> Option<(BTreeMap<u64, Gap>, u64)> {
    let mut generation = s.generation;
    if s.last_change_sequence >= unit.last_change_sequence {
        if generation < unit.generation
            || unit.last_batch_end.is_some_and(|end| end > s.observed_at)
        {
            return None;
        }
        return Some((gaps, generation));
    }
    let mut expected = s.last_change_sequence;
    for record in window
        .records
        .iter()
        .filter(|r| r.unit == s.definition.watch_ref && r.sequence > s.last_change_sequence)
    {
        if expected.checked_add(1) != Some(record.sequence) || record.generation < generation {
            return None;
        }
        gaps = gaps::apply(&gaps, &record.gaps, &record.replaced)?;
        generation = record.generation;
        expected = record.sequence;
    }
    (expected == unit.last_change_sequence).then_some((gaps, generation))
}

#[cfg(test)]
mod tests;
