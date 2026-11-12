//! Locale notifications use the same bounded, sequenced desktop event stream.

use std::time::Instant;

use du_core_contracts::settings_events::SettingsChangedBody;
use du_core_contracts::{ENVELOPE_VERSION, EventEnvelope, Id, locale_ipc::LocaleChangedBody};

use super::DesktopEvents;

impl DesktopEvents {
    /// Queues the common notification for one durable settings commit.
    pub fn publish_settings_changed(&self, body: SettingsChangedBody, correlation_id: Option<Id>) {
        let Ok(body) = serde_json::to_value(body) else {
            return;
        };
        let Some(occurred_at) = super::utc_now_timestamp() else {
            return;
        };
        self.enqueue(EventEnvelope {
            topic: "settings.changed".into(),
            contract_version: ENVELOPE_VERSION,
            seq: 0,
            correlation_id,
            job_ref: None,
            result_version: None,
            occurred_at,
            body,
        });
    }

    /// Queues a committed locale profile change through the shared desktop event path.
    ///
    /// The pending stream assigns the public sequence when it delivers the event. If
    /// several profile changes accumulate, only the newest queued profile is retained.
    pub fn publish_locale_changed(&self, body: LocaleChangedBody, correlation_id: Option<Id>) {
        let Ok(body) = serde_json::to_value(body) else {
            return;
        };
        let Some(occurred_at) = super::utc_now_timestamp() else {
            return;
        };
        let event = EventEnvelope {
            topic: "locale.changed".into(),
            contract_version: ENVELOPE_VERSION,
            seq: 0,
            correlation_id,
            job_ref: None,
            result_version: None,
            occurred_at: occurred_at.clone(),
            body,
        };
        let mut pending = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.push_latest_locale_changed(event, Instant::now(), occurred_at, || {
            super::next_loss_order(&self.1)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::EventEnvelope;
    use std::{thread, time::Duration};

    #[test]
    fn locale_changes_coalesce_and_share_sequence_and_resync_delivery()
    -> Result<(), Box<dyn std::error::Error>> {
        let correlation = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
        let desktop = DesktopEvents::default();
        desktop.begin_document_generation();
        desktop.mark_document_ready();

        for (revision, language) in [(1, "ko"), (2, "en")] {
            desktop.publish_locale_changed(
                LocaleChangedBody {
                    profile_revision:
                        du_core_contracts::locale_ipc::LocaleProfileRevision::from_u64(revision),
                    changed_fields: vec![
                        du_core_contracts::locale_ipc::LocaleChangedField::DisplayLanguage,
                    ],
                    display_language: language.into(),
                },
                Some(correlation.clone()),
            );
        }

        thread::sleep(Duration::from_millis(120));
        let wire = desktop
            .next_json()
            .ok_or("locale event was not delivered")?;
        let event: EventEnvelope = serde_json::from_str(&wire)?;
        assert_eq!(event.topic, "locale.changed");
        assert_eq!(event.seq, 1);
        assert_eq!(event.correlation_id, Some(correlation));
        assert_eq!(event.job_ref, None);
        assert_eq!(event.result_version, None);
        assert_eq!(event.body["profileRevision"], "2");
        assert_eq!(event.body["displayLanguage"], "en");
        assert!(desktop.next_json().is_none());

        // Native post loss uses the same sequence counter and emits the stream's
        // established resynchronization envelope.
        desktop.report_post_failure(wire, Vec::new());
        let recovery = desktop
            .next_json()
            .ok_or("missing event resynchronization")?;
        let recovery: EventEnvelope = serde_json::from_str(&recovery)?;
        assert_eq!(recovery.topic, "job.resync_required");
        assert_eq!(recovery.seq, 2);
        assert_eq!(recovery.body["reason"], "subscriber_delivery_failed");
        Ok(())
    }
}
