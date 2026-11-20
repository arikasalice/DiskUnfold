use super::super::contract::{Events, entry};
use super::super::events_common::{
    DocumentGate, EVENT_DELIVERY_SCRIPT, fail_event_batch, take_event_batch,
};
use super::timer::TimerSlot;
use gtk4::{gio, glib};
use std::{cell::RefCell, rc::Rc, time::Duration};
use webkit6::prelude::*;
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(16);

pub(super) struct DocumentSession {
    gate: DocumentGate,
    cancellable: gio::Cancellable,
}

impl DocumentSession {
    fn new() -> Self {
        Self {
            gate: DocumentGate::default(),
            cancellable: gio::Cancellable::new(),
        }
    }
}

pub(super) struct EventLifecycle {
    pub(super) state: Rc<RefCell<DocumentSession>>,
    events: Events,
}

impl EventLifecycle {
    pub(super) fn attach(view: &webkit6::WebView, events: Events) -> Self {
        let state = Rc::new(RefCell::new(DocumentSession::new()));
        let started_state = Rc::clone(&state);
        let started_events = events.clone();
        let ready_state = Rc::clone(&state);
        let ready_events = events.clone();

        view.connect_load_changed(move |view, load| match load {
            webkit6::LoadEvent::Started if is_trusted_document(view.uri()) => {
                let previous = {
                    let mut document = started_state.borrow_mut();
                    if !document.gate.start() {
                        return;
                    }
                    let previous = document.cancellable.clone();
                    document.cancellable = gio::Cancellable::new();
                    previous
                };
                previous.cancel();
                started_events.document_started();
            }
            webkit6::LoadEvent::Finished if is_trusted_document(view.uri()) => {
                let should_notify = ready_state.borrow_mut().gate.mark_ready();
                if should_notify {
                    ready_events.document_ready();
                }
            }
            _ => {}
        });

        Self { state, events }
    }

    pub(super) fn start_polling(&self, view: &webkit6::WebView, events: Events) -> TimerSlot {
        start_event_polling(view, Rc::clone(&self.state), events)
    }
}

impl Drop for EventLifecycle {
    fn drop(&mut self) {
        retire_session(&self.state, true);
        self.events.document_closed();
    }
}

pub(super) fn retire_session(state: &Rc<RefCell<DocumentSession>>, closed: bool) {
    let cancellable = {
        let mut document = state.borrow_mut();
        document.gate.retire(closed);
        document.cancellable.clone()
    };
    cancellable.cancel();
}

fn is_trusted_document(uri: Option<glib::GString>) -> bool {
    uri.is_some_and(|uri| uri.as_str() == entry())
}

fn event_arguments(payload: &str) -> glib::Variant {
    let arguments = glib::VariantDict::new(None);
    arguments.insert("payload", payload);
    arguments.end()
}

fn finish_event_batch(state: &Rc<RefCell<DocumentSession>>, generation: u64) {
    state.borrow_mut().gate.finish(generation);
}

fn post_event_batch(
    view: glib::WeakRef<webkit6::WebView>,
    state: Rc<RefCell<DocumentSession>>,
    events: Events,
    generation: u64,
    batch: Rc<Vec<String>>,
    index: usize,
) {
    if index == batch.len() {
        finish_event_batch(&state, generation);
        return;
    }
    let Some(view) = view.upgrade() else {
        finish_event_batch(&state, generation);
        return;
    };
    let cancellable = {
        let document = state.borrow();
        if !document.gate.is_current(generation) {
            return;
        }
        document.cancellable.clone()
    };
    if !is_trusted_document(view.uri()) {
        fail_event_batch(&events, &batch, index);
        finish_event_batch(&state, generation);
        return;
    }

    let arguments = event_arguments(&batch[index]);
    let next_view = view.downgrade();
    let source_uri = entry();
    view.call_async_javascript_function(
        EVENT_DELIVERY_SCRIPT,
        Some(&arguments),
        None,
        Some(&source_uri),
        Some(&cancellable),
        move |result| {
            if !state.borrow().gate.is_current(generation) {
                finish_event_batch(&state, generation);
                return;
            }
            if result.is_ok_and(|value| value.to_boolean()) {
                post_event_batch(next_view, state, events, generation, batch, index + 1);
            } else {
                fail_event_batch(&events, &batch, index);
                finish_event_batch(&state, generation);
            }
        },
    );
}

pub(super) fn start_event_polling(
    view: &webkit6::WebView,
    state: Rc<RefCell<DocumentSession>>,
    events: Events,
) -> TimerSlot {
    let view = view.downgrade();
    TimerSlot::every(EVENT_POLL_INTERVAL, move || {
        let (closed, generation) = {
            let document = state.borrow();
            (document.gate.closed, document.gate.ready_to_send())
        };
        if closed {
            return glib::ControlFlow::Break;
        }
        let Some(generation) = generation else {
            return glib::ControlFlow::Continue;
        };
        let Some(view) = view.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if !is_trusted_document(view.uri()) {
            return glib::ControlFlow::Continue;
        }
        let batch = Rc::new(take_event_batch(|| events.next()));
        if batch.is_empty() {
            return glib::ControlFlow::Continue;
        }
        state.borrow_mut().gate.sending = true;
        post_event_batch(
            view.downgrade(),
            Rc::clone(&state),
            events.clone(),
            generation,
            batch,
            0,
        );
        glib::ControlFlow::Continue
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVENT: &str = r#"{"contractVersion":1,"topic":"fixture.event","occurredAt":"2026-11-12T00:00:00.000Z","seq":1,"body":{"text":"\"); window.__diskunfoldInjected = true; //"}}"#;

    #[test]
    fn event_payload_is_passed_as_data() -> Result<(), Box<dyn std::error::Error>> {
        let arguments = event_arguments(EVENT);
        let dictionary = glib::VariantDict::new(Some(&arguments));
        assert_eq!(
            dictionary.lookup::<String>("payload")?.as_deref(),
            Some(EVENT)
        );
        assert!(!EVENT_DELIVERY_SCRIPT.contains("__diskunfoldInjected"));
        Ok(())
    }

    #[test]
    fn retiring_a_document_cancels_its_pending_post() {
        let state = Rc::new(RefCell::new(DocumentSession::new()));
        {
            let mut document = state.borrow_mut();
            document.gate.active = true;
            document.gate.ready = true;
            document.gate.sending = true;
            document.gate.generation = 1;
        }
        let cancellable = state.borrow().cancellable.clone();

        retire_session(&state, false);

        let document = state.borrow();
        assert!(!document.gate.active);
        assert!(!document.gate.ready);
        assert!(!document.gate.sending);
        assert!(!document.gate.closed);
        assert!(cancellable.is_cancelled());
    }
}
