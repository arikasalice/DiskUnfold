//! Fallback-chain and reason metadata resolution.

use std::collections::BTreeMap;

use du_core_contracts::locale_bundle::{
    LocaleMessageValue, LocaleMissingEntry, LocaleMissingMarker, LocaleReportMissingRequest,
    MessageTemplate, ReasonCertainty, ReasonStatus,
};

use super::{Registry, bundle_body::find_message, missing::current_time_ms, template};

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

/// Rendered copy and its translation or parameter status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedMessage {
    pub text: String,
    pub marker: Option<LocaleMissingMarker>,
    pub unknown_parameter: bool,
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
    /// Resolves a static human-readable message and annotates any fallback.
    pub fn resolve_plain_text(&self, language: &str, key: &str) -> Option<String> {
        let resolved = self.resolve_message(language, key);
        let MessageTemplate::Text {
            parameters,
            template,
        } = resolved.template
        else {
            return None;
        };
        if !parameters.is_empty() {
            return None;
        }
        let template = template::render(&template, |_| String::new()).ok()?;
        if resolved.marker.is_none() {
            return Some(template);
        }
        let marker = self.resolve_message(language, "ui.common.translation_unverified");
        if let MessageTemplate::Text {
            template: marker, ..
        } = marker.template
        {
            Some(format!("{template} ({marker})"))
        } else {
            Some(template)
        }
    }

    /// Renders a registered template with one-level nested keys and typed values.
    pub fn render_text<'a>(
        &self,
        language: &str,
        key: &str,
        values: &BTreeMap<&str, LocaleMessageValue<'a>>,
        format_value: impl Fn(LocaleMessageValue<'a>) -> Option<String>,
    ) -> RenderedMessage {
        let resolved = self.resolve_message(language, key);
        let mut unknown_parameter = false;
        let (source, parameters) = match &resolved.template {
            MessageTemplate::Text {
                template,
                parameters,
            } => (template.as_str(), parameters),
            MessageTemplate::Plural {
                variants,
                parameter,
                parameters,
            } => {
                let count = match values.get(parameter.as_str()) {
                    Some(LocaleMessageValue::Count(value)) => Some(*value),
                    _ => None,
                };
                unknown_parameter = count.is_none();
                let category = if self
                    .bundle(&resolved.language)
                    .is_some_and(|bundle| bundle.plural_rule == "en")
                    && count == Some(1)
                {
                    "one"
                } else {
                    "other"
                };
                let selected = variants.get(category).or_else(|| variants.get("other"));
                (selected.map_or(key, String::as_str), parameters)
            }
        };
        let text = template::render(source, |name| {
            let value = values
                .get(name)
                .copied()
                .filter(|value| parameters.get(name) == Some(&value.kind()));
            let rendered = value.and_then(|value| match value {
                LocaleMessageValue::Name(value) | LocaleMessageValue::Path(value) => {
                    Some(value.to_owned())
                }
                LocaleMessageValue::MessageKey(key) => {
                    self.resolve_plain_text(&resolved.language, key)
                }
                value => format_value(value),
            });
            rendered.unwrap_or_else(|| {
                unknown_parameter = true;
                self.resolve_plain_text(&resolved.language, "value.unknown")
                    .unwrap_or_else(|| "value.unknown".to_owned())
            })
        })
        .unwrap_or_else(|_| key.to_owned());
        RenderedMessage {
            text,
            marker: resolved.marker,
            unknown_parameter,
        }
    }

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
