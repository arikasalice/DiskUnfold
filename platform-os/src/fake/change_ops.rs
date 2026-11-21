//! Configurable in-memory F11 port for executor tests (FR14.HD1.app-host.DD1).
use du_core_contracts::{
    OsPath, PathEncoding,
    platform_os_change::{
        ChangeIdentity, ChangeItemObservation, ChangeOpsPort, ItemIdentityPort, TrashMoveOutcome,
        TrashOutcomeQuery,
    },
    ports::{PortError, PortErrorKind},
    volume::{Capability, CapabilityState},
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Mutex, MutexGuard},
};

/// The fake records adapter calls without exposing paths or identity bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FakeChangeCall {
    Capability,
    Lookup,
    Trash,
    Query,
}

struct State {
    items: BTreeMap<Vec<u8>, ChangeItemObservation>,
    capabilities: BTreeMap<Vec<u8>, Capability>,
    default_capability: Capability,
    move_outcomes: VecDeque<TrashMoveOutcome>,
    query_outcome: Option<TrashOutcomeQuery>,
    calls: Vec<FakeChangeCall>,
    simulated_change_calls: usize,
}

/// In-memory `ChangeOpsPort` for development and tests using the `fake` feature.
///
/// This adapter can model supported capabilities and move results. It never
/// invokes an operating-system change API.
pub struct FakeChangeOps {
    state: Mutex<State>,
}

impl FakeChangeOps {
    /// Creates a fake with the supplied path observations and unknown-volume default.
    pub fn new(items: Vec<(OsPath, ChangeItemObservation)>) -> Result<Self, PortError> {
        let mut indexed = BTreeMap::new();
        for (path, observation) in items {
            indexed.insert(path_key(&path)?, observation);
        }
        Ok(Self {
            state: Mutex::new(State {
                items: indexed,
                capabilities: BTreeMap::new(),
                default_capability: capability(CapabilityState::Unknown, Some("os.query_failed")),
                move_outcomes: VecDeque::new(),
                query_outcome: None,
                calls: Vec::new(),
                simulated_change_calls: 0,
            }),
        })
    }

    /// Sets the capability returned for one volume.
    pub fn set_capability(&self, volume_id: Vec<u8>, value: Capability) {
        self.state().capabilities.insert(volume_id, value);
    }

    /// Queues a result for the next supported fake move attempt.
    pub fn enqueue_move_outcome(&self, outcome: TrashMoveOutcome) {
        self.state().move_outcomes.push_back(outcome);
    }

    /// Sets a deterministic read-only result for the next outcome query.
    pub fn set_query_outcome(&self, outcome: TrashOutcomeQuery) {
        self.state().query_outcome = Some(outcome);
    }

    /// Replaces or inserts an item, for example to inject a race after lookup.
    pub fn replace_item(
        &self,
        path: &OsPath,
        observation: ChangeItemObservation,
    ) -> Result<(), PortError> {
        self.state().items.insert(path_key(path)?, observation);
        Ok(())
    }

    /// Removes an item from the fake namespace.
    pub fn remove_item(&self, path: &OsPath) -> Result<(), PortError> {
        self.state().items.remove(&path_key(path)?);
        Ok(())
    }

    /// Returns the observed adapter method sequence.
    pub fn calls(&self) -> Vec<FakeChangeCall> {
        self.state().calls.clone()
    }

    /// Returns the number of simulated change API calls.
    pub fn simulated_change_call_count(&self) -> usize {
        self.state().simulated_change_calls
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl ItemIdentityPort for FakeChangeOps {
    fn lookup_no_follow(&self, path: &OsPath) -> Result<ChangeItemObservation, PortError> {
        let mut state = self.state();
        state.calls.push(FakeChangeCall::Lookup);
        state
            .items
            .get(&path_key(path)?)
            .cloned()
            .ok_or_else(not_found)
    }
}

impl ChangeOpsPort for FakeChangeOps {
    fn capability(&self, volume_id: &[u8], _capability: &str) -> Capability {
        let mut state = self.state();
        state.calls.push(FakeChangeCall::Capability);
        state
            .capabilities
            .get(volume_id)
            .cloned()
            .unwrap_or_else(|| state.default_capability.clone())
    }

    fn trash_identity_bound(&self, path: &OsPath, expected: &ChangeIdentity) -> TrashMoveOutcome {
        let key = path_key(path).ok();
        let mut state = self.state();
        state.calls.push(FakeChangeCall::Trash);
        let supported = state
            .capabilities
            .get(&expected.volume_id)
            .is_some_and(|value| value.state == CapabilityState::Supported);
        if !supported {
            return TrashMoveOutcome::Refused(
                du_core_contracts::platform_os_change::TrashRefusal::Unsupported,
            );
        }
        if key
            .as_ref()
            .and_then(|key| state.items.get(key))
            .is_none_or(|current| current.identity != *expected)
        {
            return TrashMoveOutcome::Refused(
                du_core_contracts::platform_os_change::TrashRefusal::IdentityChanged,
            );
        }
        let outcome = state
            .move_outcomes
            .pop_front()
            .unwrap_or(TrashMoveOutcome::OutcomeUnknown);
        if !matches!(outcome, TrashMoveOutcome::Refused(_)) {
            state.simulated_change_calls += 1;
        }
        outcome
    }

    fn query_trash_outcome(
        &self,
        original_path: &OsPath,
        expected: &ChangeIdentity,
        _prior_location: Option<&du_core_contracts::platform_os_change::TrashLocationRef>,
    ) -> Result<TrashOutcomeQuery, PortError> {
        let mut state = self.state();
        state.calls.push(FakeChangeCall::Query);
        if let Some(outcome) = state.query_outcome.take() {
            return Ok(outcome);
        }
        Ok(match state.items.get(&path_key(original_path)?) {
            Some(current) if current.identity == *expected => TrashOutcomeQuery::NotMoved,
            _ => TrashOutcomeQuery::OutcomeUnknown,
        })
    }
}

fn capability(state: CapabilityState, reason_key: Option<&'static str>) -> Capability {
    Capability { state, reason_key }
}

fn path_key(path: &OsPath) -> Result<Vec<u8>, PortError> {
    let mut key = match &path.encoding {
        PathEncoding::Unix => vec![0],
        PathEncoding::WindowsUtf16le => vec![1],
    };
    key.extend(path.raw.bytes().map_err(|_| invalid_path())?);
    Ok(key)
}

fn invalid_path() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "path.unresolvable".into(),
        os_code: None,
    }
}

fn not_found() -> PortError {
    PortError {
        kind: PortErrorKind::NotFound,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{
        PathBytes,
        platform_os_change::{CHANGE_TRASH_IDENTITY_BOUND, TrashRefusal},
        ports::EntryKind,
    };

    fn path(value: &[u8]) -> OsPath {
        OsPath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(value),
            display: String::from_utf8_lossy(value).into_owned(),
            lossy: false,
        }
    }

    fn observation(kind: EntryKind, volume: &[u8], file: &[u8]) -> ChangeItemObservation {
        ChangeItemObservation {
            identity: ChangeIdentity {
                volume_id: volume.to_vec(),
                file_id: file.to_vec(),
                change_token: None,
            },
            kind,
            size: None,
            modified_at: None,
        }
    }

    fn value(state: CapabilityState) -> Capability {
        capability(state, Some("os.unsupported"))
    }

    fn port_result<T>(result: Result<T, PortError>) -> Result<T, Box<dyn std::error::Error>> {
        result.map_err(|error| std::io::Error::other(format!("{error:?}")).into())
    }

    #[test]
    fn unsupported_files_folders_links_and_unknown_volumes_make_zero_change_calls()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = path(b"/file");
        let folder = path(b"/folder");
        let link = path(b"/link");
        let unknown = path(b"/unknown-volume");
        let adapter = port_result(FakeChangeOps::new(vec![
            (
                file.clone(),
                observation(EntryKind::File, b"volume", b"file"),
            ),
            (
                folder.clone(),
                observation(EntryKind::Directory, b"volume", b"folder"),
            ),
            (
                link.clone(),
                observation(EntryKind::Link, b"volume", b"link"),
            ),
            (
                unknown.clone(),
                observation(EntryKind::File, b"unknown", b"file"),
            ),
        ]))?;
        adapter.set_capability(b"volume".to_vec(), value(CapabilityState::Unsupported));

        for item in [&file, &folder, &link, &unknown] {
            let expected = port_result(adapter.lookup_no_follow(item))?.identity;
            let expected_capability = if expected.volume_id == b"unknown" {
                CapabilityState::Unknown
            } else {
                CapabilityState::Unsupported
            };
            assert_eq!(
                adapter
                    .capability(&expected.volume_id, CHANGE_TRASH_IDENTITY_BOUND)
                    .state,
                expected_capability
            );
            assert_eq!(
                adapter.trash_identity_bound(item, &expected),
                TrashMoveOutcome::Refused(TrashRefusal::Unsupported)
            );
        }
        assert_eq!(adapter.simulated_change_call_count(), 0);
        Ok(())
    }

    #[test]
    fn replacement_after_lookup_is_refused_without_a_change_call()
    -> Result<(), Box<dyn std::error::Error>> {
        let item = path(b"/race");
        let original = observation(EntryKind::File, b"volume", b"before");
        let adapter = port_result(FakeChangeOps::new(vec![(item.clone(), original.clone())]))?;
        adapter.set_capability(b"volume".to_vec(), value(CapabilityState::Supported));
        let expected = port_result(adapter.lookup_no_follow(&item))?.identity;
        port_result(
            adapter.replace_item(&item, observation(EntryKind::File, b"volume", b"after")),
        )?;

        assert_eq!(
            adapter.trash_identity_bound(&item, &expected),
            TrashMoveOutcome::Refused(TrashRefusal::IdentityChanged)
        );
        assert_eq!(adapter.simulated_change_call_count(), 0);
        assert_eq!(
            adapter.calls(),
            [FakeChangeCall::Lookup, FakeChangeCall::Trash]
        );
        Ok(())
    }

    #[test]
    fn supported_outcomes_are_configurable_and_queries_remain_read_only()
    -> Result<(), Box<dyn std::error::Error>> {
        let item = path(b"/supported");
        let observed = observation(EntryKind::File, b"volume", b"id");
        let adapter = port_result(FakeChangeOps::new(vec![(item.clone(), observed.clone())]))?;
        adapter.set_capability(b"volume".to_vec(), value(CapabilityState::Supported));
        adapter.enqueue_move_outcome(TrashMoveOutcome::OutcomeUnknown);
        assert_eq!(
            adapter.trash_identity_bound(&item, &observed.identity),
            TrashMoveOutcome::OutcomeUnknown
        );
        assert_eq!(adapter.simulated_change_call_count(), 1);
        assert_eq!(
            port_result(adapter.query_trash_outcome(&item, &observed.identity, None))?,
            TrashOutcomeQuery::NotMoved
        );
        assert_eq!(adapter.simulated_change_call_count(), 1);
        Ok(())
    }
}
