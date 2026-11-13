//! Bounded, session-only missing translation collection.

use std::{
    collections::VecDeque,
    time::{SystemTime, UNIX_EPOCH},
};

use du_core_contracts::locale_bundle::{
    LocaleMissingMarker, LocaleReportMissingRequest, LocaleReportMissingResponse,
};

use super::Registry;

/// Maximum number of missing translations retained during one process session.
pub const MISSING_MESSAGE_CAPACITY: usize = 1_000;

/// One first-seen missing translation retained in session memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingMessageRecord {
    /// Missing message key.
    pub key: String,
    /// Language in which the message was missing.
    pub language: String,
    /// Whether English fallback or key display was used.
    pub marker: LocaleMissingMarker,
    /// First observation time in Unix milliseconds.
    pub first_seen_ms: u64,
}

impl Registry {
    /// Records missing translations without changing the caller's result.
    pub fn report_missing(
        &self,
        request: &LocaleReportMissingRequest,
    ) -> LocaleReportMissingResponse {
        self.report_missing_at(request, current_time_ms());
        LocaleReportMissingResponse {}
    }

    /// Returns a snapshot of the bounded, session-only missing translation list.
    pub fn missing_messages(&self) -> Vec<MissingMessageRecord> {
        self.missing_messages
            .lock()
            .map(|records| records.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub(super) fn report_missing_at(
        &self,
        request: &LocaleReportMissingRequest,
        first_seen_ms: u64,
    ) {
        if self.bundle(&request.language).is_none() {
            return;
        }
        let Ok(mut records) = self.missing_messages.lock() else {
            return;
        };
        for entry in &request.entries {
            if !valid_message_key(&entry.key) {
                continue;
            }
            if let Some(existing) = records
                .iter_mut()
                .find(|record| record.language == request.language && record.key == entry.key)
            {
                if entry.marker == LocaleMissingMarker::Key {
                    existing.marker = LocaleMissingMarker::Key;
                }
                continue;
            }
            records.push_back(MissingMessageRecord {
                key: entry.key.clone(),
                language: request.language.clone(),
                marker: entry.marker,
                first_seen_ms,
            });
            if records.len() > MISSING_MESSAGE_CAPACITY {
                let _ = records.pop_front();
            }
        }
    }
}

fn valid_message_key(key: &str) -> bool {
    if key.is_empty() || key.len() > 256 || key.starts_with('.') || key.ends_with('.') {
        return false;
    }
    let mut previous_was_dot = false;
    for byte in key.bytes() {
        if byte == b'.' {
            if previous_was_dot {
                return false;
            }
            previous_was_dot = true;
        } else if byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' {
            previous_was_dot = false;
        } else {
            return false;
        }
    }
    true
}

pub(super) fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

pub(super) type MissingMessageQueue = std::sync::Mutex<VecDeque<MissingMessageRecord>>;
