use super::model::{
    Action, Capability, CleanupTicket, Completion, Definition, Error, Interruption, Registration,
    Snapshot, State, Support,
};
use super::unit::Unit;
use crate::watch_coalescer::{Config, Gap, Key, Signal};
use std::collections::BTreeMap;

/// In-memory registry. A caller serializes changes to each unit and delivers
/// returned actions in order. No adapter calls are made by this type.
pub struct Registry {
    units: BTreeMap<[u8; 16], Unit>,
    targets: BTreeMap<([u8; 16], Key), [u8; 16]>,
    next_token: u64,
    active_limit: usize,
    retained_limit: usize,
    config: Config,
}
/// Result of admission: existing active references are reused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartResult {
    pub watch_ref: [u8; 16],
    pub already_watching: bool,
    pub actions: Vec<Action>,
}
impl Registry {
    /// Starts an empty session with injected active and retained-unit bounds.
    /// A retained bound protects gap history even for stopped or unsupported units.
    pub fn new(active_limit: usize, retained_limit: usize, config: Config) -> Result<Self, Error> {
        if active_limit == 0
            || retained_limit < active_limit
            || config.gap_limit < 2
            || config.window_ms == 0
            || config.promote_at < 2
            || config.queue_limit == 0
        {
            return Err(Error::InvalidDefinition);
        }
        Ok(Self {
            units: BTreeMap::new(),
            targets: BTreeMap::new(),
            next_token: 1,
            active_limit,
            retained_limit,
            config,
        })
    }
    /// Admits a target after the scope owner has verified its current volume.
    pub fn start(
        &mut self,
        definition: Definition,
        current_volume: &[u8],
        capability: Capability,
        at: u64,
    ) -> Result<StartResult, Error> {
        if definition.volume.is_empty() || definition.entry_path.is_empty() {
            return Err(Error::InvalidDefinition);
        }
        if definition.volume != current_volume {
            return Err(Error::ScopeChanged);
        }
        let key = (definition.scope_revision, definition.target.clone());
        if let Some(id) = self.targets.get(&key).copied() {
            return self.start_existing(id, &definition, capability, at);
        }
        if self.units.contains_key(&definition.watch_ref) {
            return Err(Error::DuplicateReference);
        }
        if self.units.len() >= self.retained_limit {
            return Err(Error::Capacity);
        }
        let at_capacity = self.active_count() >= self.active_limit;
        let supported = !at_capacity && capability.notification != Support::Unsupported;
        let token = if supported { self.allocate_token()? } else { 0 };
        let id = definition.watch_ref;
        let mut unit = Unit::new(definition, capability, (token, at), self.config);
        let actions = if at_capacity {
            unit.unsupported(at, "watch.resource_limit")
        } else if !supported {
            unit.unsupported(at, "watch.unsupported")
        } else {
            unit.initial(at)
        };
        self.units.insert(id, unit);
        self.targets.insert(key, id);
        Ok(StartResult {
            watch_ref: id,
            already_watching: false,
            actions,
        })
    }
    fn start_existing(
        &mut self,
        id: [u8; 16],
        definition: &Definition,
        capability: Capability,
        at: u64,
    ) -> Result<StartResult, Error> {
        let unit = self.units.get(&id).ok_or(Error::NotFound)?;
        if unit.definition.volume != definition.volume
            || unit.definition.entry_path != definition.entry_path
            || unit.definition.volume_path != definition.volume_path
            || unit.definition.nested_mounts != definition.nested_mounts
        {
            return Err(Error::ScopeChanged);
        }
        if matches!(
            unit.state,
            State::Starting | State::Active | State::Degraded
        ) {
            return Ok(StartResult {
                watch_ref: id,
                already_watching: true,
                actions: Vec::new(),
            });
        }
        if matches!(unit.state, State::Stopping) {
            return Err(Error::InvalidState);
        }
        let at_capacity = self.active_count() >= self.active_limit;
        let unit = self.units.get(&id).ok_or(Error::NotFound)?;
        let status_count = if at_capacity || capability.notification == Support::Unsupported {
            2
        } else {
            1
        };
        unit.check_retry(at, status_count)?;
        let actions = if at_capacity {
            self.units
                .get_mut(&id)
                .ok_or(Error::NotFound)?
                .retry_at_capacity(at)?
        } else {
            let token = if capability.notification == Support::Unsupported {
                0
            } else {
                self.allocate_token()?
            };
            self.units
                .get_mut(&id)
                .ok_or(Error::NotFound)?
                .retry(capability, token, at)?
        };
        Ok(StartResult {
            watch_ref: id,
            already_watching: false,
            actions,
        })
    }
    fn active_count(&self) -> usize {
        self.units
            .values()
            .filter(|unit| {
                matches!(
                    unit.state,
                    State::Starting | State::Active | State::Degraded | State::Stopping
                ) || (unit.state == State::Lost && unit.handle.is_some())
            })
            .count()
    }
    fn allocate_token(&mut self) -> Result<u64, Error> {
        let token = self.next_token;
        self.next_token = token.checked_add(1).ok_or(Error::CounterExhausted)?;
        Ok(token)
    }
    /// Applies a registration completion bound to its attempt and generation.
    /// The adapter must deliver each completion once. On error it retains the
    /// acquired handle and must retry, or stop and resubmit the same completion
    /// so the core can request removal.
    pub fn registration(
        &mut self,
        id: [u8; 16],
        token: u64,
        generation: u64,
        outcome: &Registration,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        self.units
            .get_mut(&id)
            .ok_or(Error::NotFound)?
            .registration(token, generation, outcome, at)
    }
    /// Accepts a prevalidated signal only for the currently active attempt and generation.
    /// On any error, the integration must fence callback intake and must not
    /// present the unit as currently observed until it has reconciled the error.
    /// A caller must retain the signal or otherwise record its observation gap.
    pub fn signal(
        &mut self,
        id: [u8; 16],
        token: u64,
        generation: u64,
        signal: &Signal,
    ) -> Result<Vec<Action>, Error> {
        self.units
            .get_mut(&id)
            .ok_or(Error::NotFound)?
            .signal(token, generation, signal)
    }
    /// Delivers a loss notification for the current attempt and generation.
    /// On error, the integration must fence intake and retain the loss evidence.
    pub fn signal_lost(
        &mut self,
        id: [u8; 16],
        token: u64,
        generation: u64,
        path: Option<&Key>,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        self.units
            .get_mut(&id)
            .ok_or(Error::NotFound)?
            .signal_lost(token, generation, path, at)
    }
    /// Flushes an elapsed coalescing window for the current attempt.
    pub fn tick(
        &mut self,
        id: [u8; 16],
        token: u64,
        generation: u64,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        self.units
            .get_mut(&id)
            .ok_or(Error::NotFound)?
            .tick(token, generation, at)
    }
    /// Fences an interrupted registration and opens an unavailable gap.
    /// On error, the integration must fence the adapter independently and must
    /// not report the unit as actively observed until the gap is recorded.
    pub fn interrupted(
        &mut self,
        id: [u8; 16],
        token: u64,
        generation: u64,
        reason: Interruption,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        self.units
            .get_mut(&id)
            .ok_or(Error::NotFound)?
            .interrupted(token, generation, reason, at)
    }
    /// Fences new signals and returns any final batches, status, and one remove command.
    pub fn stop(&mut self, id: [u8; 16], at: u64) -> Result<Vec<Action>, Error> {
        self.units.get_mut(&id).ok_or(Error::NotFound)?.stop(at)
    }
    /// Applies the result of one previously requested remove.
    pub fn removed(
        &mut self,
        id: [u8; 16],
        token: u64,
        handle: u64,
        result: Completion,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        self.units
            .get_mut(&id)
            .ok_or(Error::NotFound)?
            .removed(token, handle, result, at)
    }
    /// Returns the adapter-owned registration after a failed remove. The adapter
    /// must keep or explicitly clean up this handle and report `Removed` only
    /// after cleanup succeeds. No second remove command is issued automatically.
    pub fn cleanup_ticket(&self, id: [u8; 16]) -> Result<Option<CleanupTicket>, Error> {
        let unit = self.units.get(&id).ok_or(Error::NotFound)?;
        Ok(if unit.remove_pending && unit.remove_failed {
            unit.handle.map(|handle| CleanupTicket {
                token: unit.token,
                generation: unit.attempt_generation,
                handle,
            })
        } else {
            None
        })
    }
    /// Returns all retained gaps and both counters from one unit cut.
    /// The caller supplies the snapshot time from the same clock domain.
    pub fn incremental_snapshot(&self, id: [u8; 16], at: u64) -> Result<Snapshot, Error> {
        let unit = self.units.get(&id).ok_or(Error::NotFound)?;
        unit.check_time(at)?;
        Ok(Snapshot {
            definition: unit.definition.clone(),
            state: unit.state,
            generation: unit.generation,
            sequence: unit.sequence,
            last_change_sequence: unit.change_sequence,
            last_observed: unit.last_observed,
            observed_at: at,
            gaps: unit.gaps.clone(),
        })
    }
    /// Returns an independent copy of the current state and observation gaps.
    pub fn snapshot(&self, id: [u8; 16]) -> Result<(State, u64, Vec<Gap>), Error> {
        let unit = self.units.get(&id).ok_or(Error::NotFound)?;
        Ok((unit.state, unit.generation, unit.gaps.clone()))
    }
}
