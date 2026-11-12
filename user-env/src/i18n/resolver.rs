//! Fallback-chain and reason metadata resolution.

use std::collections::BTreeMap;

use du_core_contracts::locale_bundle::{
    LocaleMissingEntry, LocaleMissingMarker, LocaleReportMissingRequest, MessageTemplate,
    ReasonCertainty, ReasonStatus,
};

use super::{Registry, bundle_body::find_message, missing::current_time_ms};

/// Generic message key used when a reason has no owner metadata row.
pub const UNKNOWN_REASON_MESSAGE_KEY: &str = "ui.common.error_generic";

/// Localized form of one message, including a missing-translation marker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedMessage {
    /// Language supplying the resolved template.
    pub language: String,
    /// Resolved template.
    pub template: MessageTemplate,
    /// Missing marker to show with the resolved template, if any.
    pub marker: Option<LocaleMissingMarker>,
}

/// Resolved reason details; unknown reasons retain their key and no guessed action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedReason {
    /// Original reason key from the error.
    pub reason_key: String,
    /// Resolved cause or generic unknown-reason message.
    pub cause: ResolvedMessage,
    /// Resolved action for a known owner-defined reason.
    pub action: Option<ResolvedMessage>,
    /// Whether the represented operation cannot be reversed.
    pub irreversible: Option<bool>,
    /// Evidence certainty associated with the reason.
    pub certainty: Option<ReasonCertainty>,
    /// User-visible lifecycle state associated with the reason.
    pub status: Option<ReasonStatus>,
    /// Whether an owner-defined metadata row was found.
    pub known_reason: bool,
}

impl Registry {
    /// Resolves a message using the selected language, English, then the key itself.
    pub fn resolve_message(&self, language: &str, key: &str) -> ResolvedMessage {
        if let Some(bundle) = self.bundle(language)
            && let Some(message) = find_message(bundle, key)
        {
            return ResolvedMessage {
                language: language.to_owned(),
                template: message,
                marker: None,
            };
        }
        if let Some(bundle) = self.bundle("en")
            && let Some(message) = find_message(bundle, key)
        {
            self.report_missing_at(
                &LocaleReportMissingRequest {
                    language: language.to_owned(),
                    entries: vec![LocaleMissingEntry {
                        key: key.to_owned(),
                        marker: LocaleMissingMarker::Fallback,
                    }],
                },
                current_time_ms(),
            );
            return ResolvedMessage {
                language: "en".to_owned(),
                template: message,
                marker: Some(LocaleMissingMarker::Fallback),
            };
        }
        self.report_missing_at(
            &LocaleReportMissingRequest {
                language: language.to_owned(),
                entries: vec![LocaleMissingEntry {
                    key: key.to_owned(),
                    marker: LocaleMissingMarker::Key,
                }],
            },
            current_time_ms(),
        );
        ResolvedMessage {
            language: language.to_owned(),
            template: MessageTemplate::Text {
                parameters: BTreeMap::new(),
                template: key.to_owned(),
            },
            marker: Some(LocaleMissingMarker::Key),
        }
    }

    /// Resolves owner metadata or uses the published generic message for an unknown reason.
    pub fn resolve_reason(&self, language: &str, reason_key: &str) -> ResolvedReason {
        if let Some(metadata) = self.reason_messages.get(reason_key) {
            return ResolvedReason {
                reason_key: reason_key.to_owned(),
                cause: self.resolve_message(language, &metadata.cause_key),
                action: Some(self.resolve_message(language, &metadata.action_key)),
                irreversible: Some(metadata.irreversible),
                certainty: Some(metadata.certainty),
                status: Some(metadata.status),
                known_reason: true,
            };
        }
        self.report_missing_at(
            &LocaleReportMissingRequest {
                language: language.to_owned(),
                entries: vec![LocaleMissingEntry {
                    key: reason_key.to_owned(),
                    marker: LocaleMissingMarker::Key,
                }],
            },
            current_time_ms(),
        );
        ResolvedReason {
            reason_key: reason_key.to_owned(),
            cause: self.resolve_message(language, UNKNOWN_REASON_MESSAGE_KEY),
            action: None,
            irreversible: None,
            certainty: None,
            status: None,
            known_reason: false,
        }
    }
}
