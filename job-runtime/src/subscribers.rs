//! Bounded in-memory subscriber queues for job event delivery.
//!
//! This module is not installed in the desktop composition. The adapter chooses
//! which established current-value events may be coalesced; ordered events are
//! never coalesced. Rejoin validates that no publication crossed snapshot capture.
use crate::JobEvent;
use du_core_contracts::Id;
use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    sync::{Arc, Mutex},
    time::Duration,
};

const CAPACITY: usize = 1_024;
const INTERVAL: Duration = Duration::from_millis(100);

/// Supplies the existing job identity and sequence and identifies current values.
/// Returning a key permits replacement only within the same job and kind.
pub trait SubscriberEvent: Clone + Send {
    /// Distinguishes independent current-value streams of the same job.
    type Kind: Clone + Eq + Hash + Send;
    /// Returns the job and its sequence, or none for an unsequenced event.
    fn job_sequence(&self) -> Option<(&Id, u64)>;
    /// Marks only replaceable current-value events; ordered events return none.
    fn current_kind(&self) -> Option<Self::Kind>;
    /// Identifies the final event for a job.
    fn is_terminal(&self) -> bool;
}

/// Existing job events are ordered: item results must never be coalesced.
impl SubscriberEvent for JobEvent {
    type Kind = ();
    fn job_sequence(&self) -> Option<(&Id, u64)> {
        match self {
            JobEvent::StateChanged {
                job_id, sequence, ..
            }
            | JobEvent::ItemResult {
                job_id, sequence, ..
            } => Some((job_id, *sequence)),
            JobEvent::Finished {
                summary, sequence, ..
            } => Some((&summary.job_id, *sequence)),
            JobEvent::ResultVersionChanged(_) => None,
        }
    }
    fn current_kind(&self) -> Option<()> {
        None
    }
    fn is_terminal(&self) -> bool {
        matches!(self, JobEvent::Finished { .. })
    }
}

/// A queued event or a request to refresh the authoritative snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubscriberMessage<E> {
    /// An event preserved for this subscriber.
    Event(E),
    /// The queued events were discarded and an authoritative snapshot is required.
    ResyncRequired { dropped: usize },
}

/// The last sequence and terminal state included in an authoritative snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotCut {
    /// Latest job sequence represented by the snapshot.
    pub sequence: u64,
    /// Whether the represented job had reached its final state.
    pub terminal: bool,
}

struct Slot<E> {
    queue: VecDeque<E>,
    detached: bool,
    notice: bool,
    ready_at: Option<Duration>,
    ready: usize,
    cuts: HashMap<Id, SnapshotCut>,
    episode: Arc<()>,
}
impl<E> Default for Slot<E> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            detached: false,
            notice: false,
            ready_at: None,
            ready: 0,
            cuts: HashMap::new(),
            episode: Arc::new(()),
        }
    }
}
struct State<E> {
    slots: HashMap<u64, Slot<E>>,
    next: u64,
    generation: u64,
}
impl<E> Default for State<E> {
    fn default() -> Self {
        Self {
            slots: HashMap::new(),
            next: 0,
            generation: 0,
        }
    }
}

/// Fan-out gate shared by independent bounded subscriber queues.
pub struct SubscriberBus<E>(Arc<Mutex<State<E>>>, Arc<()>);
impl<E> Default for SubscriberBus<E> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(State::default())), Arc::new(()))
    }
}

/// An independent subscriber; dropping it unregisters its queue.
pub struct Subscription<E> {
    state: Arc<Mutex<State<E>>>,
    owner: Arc<()>,
    id: u64,
}

impl<E: SubscriberEvent> SubscriberBus<E> {
    pub(crate) fn owns(&self, subscription: &Subscription<E>) -> bool {
        Arc::ptr_eq(&self.1, &subscription.owner)
    }
    /// Register an empty queue. Failure indicates a poisoned gate or exhausted IDs.
    pub fn subscribe(&self) -> Option<Subscription<E>> {
        let mut state = self.0.lock().ok()?;
        let id = state.next.checked_add(1)?;
        state.next = id;
        state.slots.insert(id, Slot::default());
        Some(Subscription {
            state: self.0.clone(),
            owner: self.1.clone(),
            id,
        })
    }

    /// Publish an event without invoking or waiting for subscriber callbacks.
    /// Calls must be serialized in per-job sequence order by the producer.
    pub fn publish(&self, event: E, now: Duration) -> bool {
        let Ok(mut state) = self.0.lock() else {
            return false;
        };
        let Some(generation) = state.generation.checked_add(1) else {
            return false;
        };
        state.generation = generation;
        for slot in state.slots.values_mut() {
            slot.push(event.clone(), now);
        }
        true
    }

    /// Forgets the sequence guard once the bound runtime has drained delivery.
    /// Queued events have already passed the guard and retain their delivery order.
    pub(crate) fn retire_job(&self, id: &Id) -> bool {
        let Ok(mut state) = self.0.lock() else {
            return false;
        };
        for slot in state.slots.values_mut() {
            slot.cuts.remove(id);
        }
        true
    }
}

impl<E: SubscriberEvent> Slot<E> {
    fn push(&mut self, event: E, now: Duration) {
        if self.detached {
            return;
        }
        if self.ready_at.is_some_and(|deadline| now >= deadline) {
            self.ready = self.queue.len();
            self.ready_at = None;
        }
        if let Some((job, sequence)) = event.job_sequence() {
            if sequence == 0
                || self
                    .cuts
                    .get(job)
                    .is_some_and(|cut| cut.terminal || sequence <= cut.sequence)
            {
                return;
            }
            self.cuts.insert(
                job.clone(),
                SnapshotCut {
                    sequence,
                    terminal: event.is_terminal(),
                },
            );
            if !event.is_terminal()
                && let Some(kind) = event.current_kind()
                && let Some(relative) = self.queue.iter().skip(self.ready).position(|previous| {
                    previous.job_sequence().is_some_and(|(old, _)| old == job)
                        && previous.current_kind().as_ref() == Some(&kind)
                })
            {
                let index = relative + self.ready;
                self.queue.remove(index);
            }
        }
        self.queue.push_back(event);
        if self.queue.len() >= CAPACITY {
            self.queue.clear();
            self.ready = 0;
            self.ready_at = None;
            self.detached = true;
            self.notice = true;
            self.episode = Arc::new(());
        } else if self.ready_at.is_none() {
            self.ready_at = Some(now.saturating_add(INTERVAL));
        }
    }
    fn pop(&mut self, now: Duration) -> Option<SubscriberMessage<E>> {
        if self.notice {
            self.notice = false;
            return Some(SubscriberMessage::ResyncRequired { dropped: CAPACITY });
        }
        if self.detached {
            return None;
        }
        if self.ready == 0 {
            if self.ready_at.is_none_or(|deadline| now < deadline) {
                return None;
            }
            self.ready = self.queue.len();
            self.ready_at = None;
        }
        let item = self.queue.pop_front()?;
        self.ready -= 1;
        Some(SubscriberMessage::Event(item))
    }
}

impl<E: SubscriberEvent> Subscription<E> {
    #[cfg(test)]
    pub(crate) fn retained_cut_count(&self) -> Option<usize> {
        Some(self.state.lock().ok()?.slots.get(&self.id)?.cuts.len())
    }

    pub(crate) fn is_poisoned(&self) -> bool {
        self.state.is_poisoned()
    }
    /// Pop an eligible event without invoking producer work or waiting for data.
    pub fn try_next(&self, now: Duration) -> Option<SubscriberMessage<E>> {
        self.state.lock().ok()?.slots.get_mut(&self.id)?.pop(now)
    }

    /// Forget a terminal job after the runtime has retired it and no late report can exist.
    /// Returns false while its events are queued or the subscriber is detached.
    pub fn forget_retired_job(&self, job: &Id) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(slot) = state.slots.get_mut(&self.id) else {
            return false;
        };
        if slot.detached
            || !slot.cuts.get(job).is_some_and(|cut| cut.terminal)
            || slot
                .queue
                .iter()
                .any(|event| event.job_sequence().is_some_and(|(id, _)| id == job))
        {
            return false;
        }
        slot.cuts.remove(job);
        true
    }

    /// Begin a snapshot attempt while detached. Capture the snapshot after this call.
    /// The composition must fence upstream deliveries before capturing unsequenced sources.
    pub fn begin_rejoin(&self) -> Option<RejoinToken> {
        let state = self.state.lock().ok()?;
        let slot = state.slots.get(&self.id)?;
        if !slot.detached {
            return None;
        }
        Some(RejoinToken {
            subscriber: self.id,
            generation: state.generation,
            owner: self.owner.clone(),
            episode: slot.episode.clone(),
        })
    }

    /// Rejoin only if no publication crossed the snapshot attempt.
    /// A false result requires a new token and a fresh authoritative snapshot.
    /// The cuts must cover every job in the authoritative snapshot, including terminal
    /// jobs, and the snapshot must also cover any unsequenced event sources.
    pub fn complete_rejoin(&self, token: RejoinToken, cuts: HashMap<Id, SnapshotCut>) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if !Arc::ptr_eq(&token.owner, &self.owner)
            || token.subscriber != self.id
            || token.generation != state.generation
        {
            return false;
        }
        let Some(slot) = state.slots.get_mut(&self.id) else {
            return false;
        };
        if !slot.detached || !Arc::ptr_eq(&token.episode, &slot.episode) {
            return false;
        }
        slot.cuts = cuts;
        slot.detached = false;
        slot.notice = false;
        true
    }
}

impl Subscription<JobEvent> {
    /// Atomically captures the bound runtime's job state and rejoins this queue.
    /// The caller must establish that this subscription belongs to that runtime's bus.
    pub(crate) fn rejoin_runtime(
        &self,
        runtime: &crate::JobRuntime,
        token: RejoinToken,
    ) -> Result<Option<HashMap<Id, RuntimeJobCut>>, crate::RuntimeError> {
        if !Arc::ptr_eq(&token.owner, &self.owner) || token.subscriber != self.id {
            return Ok(None);
        }
        // The job table is always acquired before the subscriber gate. Delivery
        // calls the bus without holding the table and never invokes callbacks under it.
        let table = runtime
            .table
            .lock()
            .map_err(|_| crate::RuntimeError::Poisoned)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| crate::RuntimeError::DeliveryUnavailable)?;
        let Some(slot) = state.slots.get_mut(&self.id) else {
            return Ok(None);
        };
        if !slot.detached || !Arc::ptr_eq(&token.episode, &slot.episode) {
            return Ok(None);
        }
        if table
            .jobs
            .values()
            .any(|job| job.state.is_terminal() && job.terminal.is_none())
        {
            return Err(crate::RuntimeError::DeliveryBusy);
        }
        let snapshot: HashMap<_, _> = table
            .jobs
            .iter()
            .map(|(id, job)| {
                let terminal = job.terminal_snapshot();
                (
                    id.clone(),
                    RuntimeJobCut {
                        snapshot: job.snapshot(),
                        summary: job.summary.clone(),
                        history_recorded: terminal.as_ref().map(|result| result.history_recorded),
                        history_warning: terminal.and_then(|result| result.warning),
                    },
                )
            })
            .collect();
        slot.cuts = snapshot
            .iter()
            .map(|(id, row)| {
                (
                    id.clone(),
                    SnapshotCut {
                        sequence: row.snapshot.sequence,
                        terminal: row.snapshot.state.is_terminal(),
                    },
                )
            })
            .collect();
        slot.detached = false;
        slot.notice = false;
        Ok(Some(snapshot))
    }
}

/// A job state and terminal summary captured at the same runtime cut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeJobCut {
    /// The state and job sequence at the cut.
    pub snapshot: crate::JobSnapshot,
    /// The terminal summary, if the job has finished.
    pub summary: Option<du_core_contracts::job::JobSummary>,
    /// Whether the terminal outcome was recorded; absent for nonterminal jobs.
    pub history_recorded: Option<bool>,
    /// The history warning emitted with the terminal event, if any.
    pub history_warning: Option<String>,
}

/// Identifies one subscriber's publication-free snapshot attempt.
pub struct RejoinToken {
    subscriber: u64,
    generation: u64,
    owner: Arc<()>,
    episode: Arc<()>,
}

impl<E> Drop for Subscription<E> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.slots.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests;
