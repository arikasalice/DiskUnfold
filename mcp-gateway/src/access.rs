//! In-process admission and response queue state for an MCP transport.
//!
//! This module owns request admission, cancellation flags, and responses that
//! have not yet been handed to a transport. It does not own a listening socket;
//! a concrete transport must close its acceptor and sockets when this gate is
//! closed.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// A fail-closed gate shared by the MCP acceptor and its connection handlers.
#[derive(Clone)]
pub struct AccessGate(Arc<Mutex<GateState>>);

#[derive(Default)]
struct GateState {
    accepting: bool,
    next_session: u64,
    sessions: BTreeMap<u64, SessionState>,
    close_report: Option<AccessCloseReport>,
}

#[derive(Default)]
struct SessionState {
    open: bool,
    next_request: u64,
    requests: BTreeMap<u64, Arc<AtomicBool>>,
    responses: VecDeque<Vec<u8>>,
}

/// Counts work synchronously retired by the first access-close call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessCloseReport {
    /// Sessions that were open when the gate closed.
    pub sessions_closed: usize,
    /// Queued responses discarded before a transport took ownership of them.
    pub responses_discarded: usize,
}

/// One connection registered before shutdown admission closed.
pub struct Session {
    gate: Arc<Mutex<GateState>>,
    id: u64,
}

/// A cooperative cancellation flag bound to one session request.
pub struct RequestPermit {
    gate: Arc<Mutex<GateState>>,
    session_id: u64,
    request_id: u64,
    cancelled: Arc<AtomicBool>,
}

/// Access-state failure. A poisoned lock is treated as unavailable, which is
/// fail-closed for every admission and response operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessError {
    /// Shutdown has closed access or the state lock is unavailable.
    Closed,
    /// This connection has already closed.
    SessionClosed,
    /// The permit was cancelled by shutdown or the connection.
    RequestCancelled,
    /// The request permit belongs to a different connection.
    ForeignRequest,
    /// The monotonic session or request counter cannot be advanced.
    IdentifierExhausted,
}

impl Default for AccessGate {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(GateState {
            accepting: true,
            ..GateState::default()
        })))
    }
}

impl AccessGate {
    /// Registers one connection while access remains open.
    pub fn connect(&self) -> Result<Session, AccessError> {
        let mut state = self.0.lock().map_err(|_| AccessError::Closed)?;
        if !state.accepting {
            return Err(AccessError::Closed);
        }
        let id = state
            .next_session
            .checked_add(1)
            .ok_or(AccessError::IdentifierExhausted)?;
        state.next_session = id;
        state.sessions.insert(
            id,
            SessionState {
                open: true,
                ..SessionState::default()
            },
        );
        Ok(Session {
            gate: self.0.clone(),
            id,
        })
    }

    /// Atomically denies new connections and requests, cancels active requests,
    /// closes registered sessions, and drops responses still queued for send.
    /// Repeated calls return the first close report.
    pub fn close_access(&self) -> AccessCloseReport {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(report) = state.close_report {
            return report;
        }

        state.accepting = false;
        let mut report = AccessCloseReport::default();
        for session in state.sessions.values_mut() {
            if session.open {
                report.sessions_closed = report.sessions_closed.saturating_add(1);
            }
            session.open = false;
            for cancelled in session.requests.values() {
                cancelled.store(true, Ordering::Release);
            }
            report.responses_discarded = report
                .responses_discarded
                .saturating_add(session.responses.len());
            session.responses.clear();
        }
        state.close_report = Some(report);
        report
    }

    /// Reports whether the gate is open. Lock poisoning is treated as closed.
    pub fn is_accepting(&self) -> bool {
        self.0.lock().is_ok_and(|state| state.accepting)
    }
}

impl Session {
    /// Starts one request on this connection unless shutdown or cancellation won.
    pub fn begin_request(&self) -> Result<RequestPermit, AccessError> {
        let mut state = self.gate.lock().map_err(|_| AccessError::Closed)?;
        if !state.accepting {
            return Err(AccessError::Closed);
        }
        let session = state
            .sessions
            .get_mut(&self.id)
            .filter(|session| session.open)
            .ok_or(AccessError::SessionClosed)?;
        let request_id = session
            .next_request
            .checked_add(1)
            .ok_or(AccessError::IdentifierExhausted)?;
        session.next_request = request_id;
        let cancelled = Arc::new(AtomicBool::new(false));
        session.requests.insert(request_id, cancelled.clone());
        Ok(RequestPermit {
            gate: self.gate.clone(),
            session_id: self.id,
            request_id,
            cancelled,
        })
    }

    /// Queues a response only while this request can still be delivered.
    ///
    /// A transport must serialize taking queued responses with its socket write
    /// and close the socket as part of its own shutdown adapter.
    pub fn queue_response(
        &self,
        request: &RequestPermit,
        response: Vec<u8>,
    ) -> Result<(), AccessError> {
        if request.session_id != self.id || !Arc::ptr_eq(&request.gate, &self.gate) {
            return Err(AccessError::ForeignRequest);
        }
        if request.is_cancelled() {
            return Err(AccessError::RequestCancelled);
        }
        let mut state = self.gate.lock().map_err(|_| AccessError::Closed)?;
        if !state.accepting {
            return Err(AccessError::Closed);
        }
        let session = state
            .sessions
            .get_mut(&self.id)
            .filter(|session| session.open)
            .ok_or(AccessError::SessionClosed)?;
        if request.is_cancelled() || !session.requests.contains_key(&request.request_id) {
            return Err(AccessError::RequestCancelled);
        }
        session.responses.push_back(response);
        Ok(())
    }

    /// Takes the next queued response for transport delivery.
    pub fn take_response(&self) -> Result<Option<Vec<u8>>, AccessError> {
        let mut state = self.gate.lock().map_err(|_| AccessError::Closed)?;
        if !state.accepting {
            return Err(AccessError::Closed);
        }
        let session = state
            .sessions
            .get_mut(&self.id)
            .filter(|session| session.open)
            .ok_or(AccessError::SessionClosed)?;
        Ok(session.responses.pop_front())
    }

    /// Closes this connection, cancels its requests, and drops its queued responses.
    pub fn close(&self) {
        close_session(&self.gate, self.id);
    }
}

impl RequestPermit {
    /// Returns whether the connection or gateway requested cooperative cancellation.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Drop for RequestPermit {
    fn drop(&mut self) {
        let Ok(mut state) = self.gate.lock() else {
            return;
        };
        if let Some(session) = state.sessions.get_mut(&self.session_id) {
            session.requests.remove(&self.request_id);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        close_session(&self.gate, self.id);
    }
}

fn close_session(gate: &Mutex<GateState>, id: u64) {
    let Ok(mut state) = gate.lock() else {
        return;
    };
    if let Some(mut session) = state.sessions.remove(&id) {
        session.open = false;
        for cancelled in session.requests.values() {
            cancelled.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn closes_admission_cancels_requests_and_discards_queued_responses() -> Result<(), String> {
        let gate = AccessGate::default();
        let session = gate
            .connect()
            .map_err(|error| format!("connect: {error:?}"))?;
        let request = session
            .begin_request()
            .map_err(|error| format!("begin request: {error:?}"))?;
        session
            .queue_response(&request, vec![1, 2, 3])
            .map_err(|error| format!("queue response: {error:?}"))?;

        let report = gate.close_access();

        assert_eq!(
            report,
            AccessCloseReport {
                sessions_closed: 1,
                responses_discarded: 1,
            }
        );
        assert!(request.is_cancelled());
        assert!(!gate.is_accepting());
        assert_eq!(gate.close_access(), report);
        assert_eq!(gate.connect().err(), Some(AccessError::Closed));
        assert_eq!(session.begin_request().err(), Some(AccessError::Closed));
        assert_eq!(session.take_response().err(), Some(AccessError::Closed));
        Ok(())
    }

    #[test]
    fn a_response_taken_before_close_is_not_counted_as_unsent() -> Result<(), String> {
        let gate = AccessGate::default();
        let session = gate
            .connect()
            .map_err(|error| format!("connect: {error:?}"))?;
        let request = session
            .begin_request()
            .map_err(|error| format!("begin request: {error:?}"))?;
        session
            .queue_response(&request, vec![9])
            .map_err(|error| format!("queue response: {error:?}"))?;
        assert_eq!(
            session
                .take_response()
                .map_err(|error| format!("take: {error:?}"))?,
            Some(vec![9])
        );
        assert_eq!(gate.close_access().responses_discarded, 0);
        Ok(())
    }

    #[test]
    fn connection_admission_racing_close_has_one_ordered_outcome() -> Result<(), String> {
        let gate = AccessGate::default();
        let barrier = Arc::new(Barrier::new(2));
        let racing_gate = gate.clone();
        let racing_barrier = barrier.clone();
        let racer = std::thread::spawn(move || {
            racing_barrier.wait();
            racing_gate.connect().ok()
        });
        barrier.wait();
        let report = gate.close_access();
        let admitted = racer.join().map_err(|_| "admission racer panicked")?;
        assert_eq!(report.sessions_closed, usize::from(admitted.is_some()));
        assert!(!gate.is_accepting());
        drop(admitted);
        Ok(())
    }
}
