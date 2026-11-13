use super::super::contract::{EventPostFailure, Events, entry};
use super::timer::TimerSlot;
use gtk4::{gio, glib};
use std::{cell::RefCell, rc::Rc, time::Duration};
use webkit6::prelude::*;
const EVENT_BATCH_LIMIT: usize = 32;
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const EVENT_DELIVERY_SCRIPT: &str = r#"
    if (location.href !== 'diskunfold://app/index.html' || window !== window.top) return false;
    let detail;
    try { detail = JSON.parse(payload); } catch { return false; }
    window.dispatchEvent(new CustomEvent('du://event', { detail }));
    return true;
"#;

pub(super) struct DocumentSession {
    active: bool,
    ready: bool,
    closed: bool,
    generation: u64,
    sending: bool,
    cancellable: gio::Cancellable,
}

impl DocumentSession {
    fn new() -> Self {
        Self {
            active: false,
            ready: false,
            closed: false,
            generation: 0,
            sending: false,
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
                    if document.closed {
                        return;
                    }
                    let previous = document.cancellable.clone();
                    document.generation = document.generation.wrapping_add(1);
                    document.active = true;
                    document.ready = false;
                    document.sending = false;
                    document.cancellable = gio::Cancellable::new();
                    previous
                };
                previous.cancel();
                started_events.document_started();
            }
            webkit6::LoadEvent::Finished if is_trusted_document(view.uri()) => {
                let should_notify = {
                    let mut document = ready_state.borrow_mut();
                    if document.closed || !document.active || document.ready {
                        false
                    } else {
                        document.ready = true;
                        true
                    }
                };
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
        document.active = false;
        document.ready = false;
        document.closed |= closed;
        document.sending = false;
        document.cancellable.clone()
    };
    cancellable.cancel();
}

fn is_trusted_document(uri: Option<glib::GString>) -> bool {
    uri.is_some_and(|uri| uri.as_str() == entry())
}

fn take_event_batch(mut next: impl FnMut() -> Option<String>) -> Vec<String> {
    std::iter::from_fn(&mut next)
        .take(EVENT_BATCH_LIMIT)
        .collect()
}

fn event_arguments(payload: &str) -> glib::Variant {
    let arguments = glib::VariantDict::new(None);
    arguments.insert("payload", payload);
    arguments.end()
}

fn finish_event_batch(state: &Rc<RefCell<DocumentSession>>, generation: u64) {
    let mut document = state.borrow_mut();
    if document.generation == generation {
        document.sending = false;
    }
}

fn fail_event_batch(events: &Events, batch: &[String], index: usize) {
    let Some(failed) = batch.get(index) else {
        return;
    };
    events.report_post_failure(EventPostFailure {
        failed: failed.clone(),
        discarded: batch[index + 1..].to_vec(),
    });
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
        if document.closed
            || !document.active
            || !document.ready
            || document.generation != generation
        {
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
            let still_current = {
                let document = state.borrow();
                !document.closed
                    && document.active
                    && document.ready
                    && document.generation == generation
            };
            if !still_current {
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
        let (closed, active, ready, sending, generation) = {
            let document = state.borrow();
            (
                document.closed,
                document.active,
                document.ready,
                document.sending,
                document.generation,
            )
        };
        if closed {
            return glib::ControlFlow::Break;
        }
        if !active || !ready || sending {
            return glib::ControlFlow::Continue;
        }
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
        state.borrow_mut().sending = true;
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
    fn event_batch_is_bounded() {
        let mut next = (0..EVENT_BATCH_LIMIT + 1).map(|index| format!("event-{index}"));
        let batch = take_event_batch(|| next.next());
        assert_eq!(batch.len(), EVENT_BATCH_LIMIT);
        assert_eq!(next.next().as_deref(), Some("event-32"));
    }

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
    fn post_failure_reports_the_failed_envelope_and_later_batch_items()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::{Arc, Mutex};

        let reported = Arc::new(Mutex::new(None));
        let callback = Arc::clone(&reported);
        let events = Events::new(
            || None,
            super::super::super::contract::EventCallbacks::new(
                move |failure| {
                    if let Ok(mut reported) = callback.lock() {
                        *reported = Some(failure);
                    }
                },
                || {},
                || {},
                || {},
            ),
        );
        let batch = vec!["first".into(), "failed".into(), "discarded".into()];
        fail_event_batch(&events, &batch, 1);
        let failure = reported.lock().map_err(|_| "post failure lock")?;
        let expected = Some(EventPostFailure {
            failed: "failed".into(),
            discarded: vec!["discarded".into()],
        });
        assert_eq!(failure.as_ref(), expected.as_ref());
        Ok(())
    }
}
