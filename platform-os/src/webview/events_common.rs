//! Event delivery rules shared by the WebKit adapters (Linux and macOS).
//!
//! Both adapters poll the host's event source on the UI thread, take a bounded batch and
//! post it to the trusted top-level document one envelope at a time, in order. This module
//! holds the parts that do not depend on the WebKit binding: the batch bound, the page
//! function that receives an envelope, the per-document gate and the failure report.
use super::contract::{EventPostFailure, Events};

/// Most envelopes taken from the source for one post.
pub(super) const EVENT_BATCH_LIMIT: usize = 32;

/// Body of the page function that receives one envelope as its `payload` argument. The
/// envelope travels as data and never as script text, so its content cannot run.
pub(super) const EVENT_DELIVERY_SCRIPT: &str = r#"
    if (location.href !== 'diskunfold://app/index.html' || window !== window.top) return false;
    let detail;
    try { detail = JSON.parse(payload); } catch { return false; }
    window.dispatchEvent(new CustomEvent('du://event', { detail }));
    return true;
"#;

/// Whether events may be posted to the current top-level document.
///
/// A trusted document that starts loading opens a new generation; it accepts events once
/// it finished loading. A post in flight belongs to its generation, so a completion that
/// arrives after the next document started cannot clear the new document's state.
#[derive(Debug, Default)]
pub(super) struct DocumentGate {
    pub(super) active: bool,
    pub(super) ready: bool,
    pub(super) closed: bool,
    pub(super) generation: u64,
    pub(super) sending: bool,
}

impl DocumentGate {
    /// A trusted document started loading; false once the window closed.
    pub(super) fn start(&mut self) -> bool {
        if self.closed {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.active = true;
        self.ready = false;
        self.sending = false;
        true
    }

    /// The started document finished loading; true only the first time per generation.
    pub(super) fn mark_ready(&mut self) -> bool {
        if self.closed || !self.active || self.ready {
            return false;
        }
        self.ready = true;
        true
    }

    /// The document went away; `closed` also ends delivery for good.
    pub(super) fn retire(&mut self, closed: bool) {
        self.active = false;
        self.ready = false;
        self.closed |= closed;
        self.sending = false;
    }

    /// The generation a new batch may be posted to, when no post is in flight.
    pub(super) fn ready_to_send(&self) -> Option<u64> {
        (!self.closed && self.active && self.ready && !self.sending).then_some(self.generation)
    }

    /// Whether a post for `generation` may continue.
    pub(super) fn is_current(&self, generation: u64) -> bool {
        !self.closed && self.active && self.ready && self.generation == generation
    }

    /// Ends the post of `generation`; a stale completion changes nothing.
    pub(super) fn finish(&mut self, generation: u64) {
        if self.generation == generation {
            self.sending = false;
        }
    }
}

/// Takes at most [`EVENT_BATCH_LIMIT`] envelopes from the source.
pub(super) fn take_event_batch(mut next: impl FnMut() -> Option<String>) -> Vec<String> {
    std::iter::from_fn(&mut next)
        .take(EVENT_BATCH_LIMIT)
        .collect()
}

/// Reports the envelope at `index` as failed and the later ones of its batch as discarded.
pub(super) fn fail_event_batch(events: &Events, batch: &[String], index: usize) {
    let Some(failed) = batch.get(index) else {
        return;
    };
    events.report_post_failure(EventPostFailure {
        failed: failed.clone(),
        discarded: batch[index + 1..].to_vec(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_batch_is_bounded() {
        let mut next = (0..EVENT_BATCH_LIMIT + 1).map(|index| format!("event-{index}"));
        let batch = take_event_batch(|| next.next());
        assert_eq!(batch.len(), EVENT_BATCH_LIMIT);
        assert_eq!(next.next().as_deref(), Some("event-32"));
    }

    #[test]
    fn post_failure_reports_the_failed_envelope_and_later_batch_items()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::{Arc, Mutex};

        let reported = Arc::new(Mutex::new(None));
        let callback = Arc::clone(&reported);
        let events = Events::new(
            || None,
            super::super::contract::EventCallbacks::new(
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

    #[test]
    fn a_document_takes_events_only_between_ready_and_retire() {
        let mut gate = DocumentGate::default();
        assert_eq!(gate.ready_to_send(), None);
        assert!(gate.start());
        assert_eq!(gate.ready_to_send(), None);
        assert!(gate.mark_ready());
        assert!(!gate.mark_ready(), "ready is reported once per generation");
        assert_eq!(gate.ready_to_send(), Some(1));
        gate.sending = true;
        assert_eq!(gate.ready_to_send(), None);
        gate.retire(false);
        assert!(!gate.is_current(1) && !gate.sending);
        gate.retire(true);
        assert!(!gate.start(), "a closed window takes no new document");
    }

    #[test]
    fn stale_completion_cannot_clear_a_new_documents_send() {
        let mut gate = DocumentGate {
            generation: 2,
            sending: true,
            ..DocumentGate::default()
        };
        gate.finish(1);
        assert!(gate.sending);
        gate.finish(2);
        assert!(!gate.sending);
    }
}
