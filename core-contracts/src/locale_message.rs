//! F13 message resolution port (FR5.HD2.app-host.DD1).
//!
//! The types here are in-process Rust contracts only. They carry no `serde` or
//! `ts_rs` derives, so they never appear in the IPC wire format or in the
//! generated TypeScript. The wire shapes for bundles and missing-translation
//! reports stay in [`crate::locale_bundle`] and [`crate::locale_ipc`].

use std::collections::BTreeMap;

use crate::locale_bundle::{LocaleMessageValue, LocaleMissingMarker};

/// One request to render a human-readable message.
///
/// The parameter map is keyed by template parameter name. Each value is typed
/// raw input; the renderer, not the caller, turns it into text.
#[derive(Clone, Copy, Debug)]
pub struct LocaleMessageRequest<'a> {
    /// Language the user selected, for example `ko`.
    pub language: &'a str,
    /// Stable message key, for example `ui.history.records_shown`.
    pub key: &'a str,
    /// Typed values for the named template parameters.
    pub parameters: &'a BTreeMap<&'a str, LocaleMessageValue<'a>>,
}

/// Text produced by a [`LocaleMessagePort`] with its translation and parameter status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedLocaleMessage {
    /// Final sentence, ready to display or place in an output.
    pub text: String,
    /// `None` when the selected language supplied the message,
    /// [`LocaleMissingMarker::Fallback`] when the baseline English message was
    /// used, and [`LocaleMissingMarker::Key`] when the key itself is the text.
    pub marker: Option<LocaleMissingMarker>,
    /// `true` when at least one parameter was missing, had a kind that differs
    /// from the template's declaration, or could not be formatted, and
    /// `value.unknown` text was substituted for it.
    pub unknown_parameter: bool,
}

/// Resolves a message key, typed parameters and a language into a sentence.
///
/// Engine business components never build sentences themselves. They hand the
/// key and raw values to an implementation of this port at the composition
/// boundary, so every language-dependent word and number format has one owner.
///
/// # Failure behavior
///
/// `render` cannot fail and never panics on caller input:
///
/// - A key that no registered language knows renders as the key string with
///   [`LocaleMissingMarker::Key`].
/// - A key missing from the selected language but present in the baseline
///   English bundle renders the English message with
///   [`LocaleMissingMarker::Fallback`].
/// - A missing parameter, or one whose kind differs from the template's
///   declaration, renders as the `value.unknown` message and sets
///   `unknown_parameter`. Extra parameters are ignored.
///
/// # Value formatting
///
/// Counts, sizes, timestamps and numbers are formatted by the implementation
/// with its own locale format tables. The port takes no formatting callback,
/// which keeps it object safe (`&dyn LocaleMessagePort`). An implementation
/// that needs a format profile (number and date formats, time zone policy,
/// size units) receives it when it is constructed, not per request.
///
/// # Sharing
///
/// [`LocaleMessageValue::Path`] and [`LocaleMessageValue::Name`] are inserted
/// exactly as given. The port does not redact, shorten or mask them. A caller
/// that places rendered text in a report, export or diagnostic bundle must have
/// the `share-policy` and `export-render` components decide whether those
/// values may leave the application before it calls `render`.
pub trait LocaleMessagePort: Send + Sync {
    /// Renders `request.key` in `request.language` with the typed parameters.
    ///
    /// Implementations may record missing translations as a side effect, but
    /// that never changes the returned value.
    fn render(&self, request: &LocaleMessageRequest<'_>) -> RenderedLocaleMessage;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl LocaleMessagePort for Echo {
        fn render(&self, request: &LocaleMessageRequest<'_>) -> RenderedLocaleMessage {
            RenderedLocaleMessage {
                text: request.key.to_owned(),
                marker: Some(LocaleMissingMarker::Key),
                unknown_parameter: !request.parameters.is_empty(),
            }
        }
    }

    #[test]
    fn port_is_object_safe_and_shareable_across_threads() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn LocaleMessagePort>();

        let parameters = BTreeMap::from([("count", LocaleMessageValue::Count(2))]);
        let port: &dyn LocaleMessagePort = &Echo;
        let rendered = port.render(&LocaleMessageRequest {
            language: "ko",
            key: "ui.fixture.key",
            parameters: &parameters,
        });
        assert_eq!(rendered.text, "ui.fixture.key");
        assert_eq!(rendered.marker, Some(LocaleMissingMarker::Key));
        assert!(rendered.unknown_parameter);
    }
}
