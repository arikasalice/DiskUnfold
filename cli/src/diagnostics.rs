//! Human-readable stderr diagnostics: error lines and translation-fallback records.
//!
//! Everything here is text for people. Machine-readable output never passes
//! through this module or through the message resolver.

use std::io::Write;

use du_core_contracts::ErrorCode;
use du_user_env::i18n::Registry;

/// Stable reason key of the warning written when a message used a fallback.
pub(crate) const TRANSLATION_FALLBACK_REASON_KEY: &str = "locale.translation_fallback";
/// Message key of the language-neutral cause text for the fallback warning.
const TRANSLATION_FALLBACK_CAUSE_KEY: &str = "reason.locale.translation_fallback.cause";

/// Writes a diagnostic line without panicking when stderr has been closed.
pub(crate) fn write_stderr_line(stderr: &mut impl Write, message: &str) {
    let _ = writeln!(stderr, "{message}");
}

/// Returns the English wire name of an error code, for example `INVALID_ARGUMENT`.
fn error_code_name(code: &ErrorCode) -> String {
    match serde_json::to_value(code) {
        Ok(serde_json::Value::String(name)) => name,
        _ => "INTERNAL".to_owned(),
    }
}

/// Writes one error line: the stable reason key, the localized text when one
/// could be resolved, and the English error code. The line never carries
/// parameter values, so it is safe to forward unchanged.
pub(crate) fn write_error_line(
    stderr: &mut impl Write,
    reason_key: &str,
    code: &ErrorCode,
    localized_text: Option<&str>,
) {
    let code = error_code_name(code);
    let line = match localized_text {
        Some(text) => format!("DiskUnfold: {reason_key}: {text} [{code}]"),
        None => format!("DiskUnfold: {reason_key} [{code}]"),
    };
    write_stderr_line(stderr, &line);
}

/// Message key of the one-line summary that replaces the warnings beyond the cap.
const TRANSLATION_FALLBACK_OMITTED_KEY: &str = "cli.diagnostics.translation_fallback_omitted";

/// Largest number of per-key `locale.translation_fallback` warnings written in one run.
///
/// The session list can hold [`du_user_env::i18n::MISSING_MESSAGE_CAPACITY`] (1,000) records. Writing
/// all of them would bury the command's own diagnostics in text meant for
/// people. A run that falls back for more than a handful of keys has a broadly
/// incomplete bundle, so the first records are enough to name the cause and a
/// count of the rest says how far it goes. The registry still holds every
/// record for a diagnostics export.
pub(crate) const MAX_TRANSLATION_FALLBACK_LINES: usize = 20;

/// Writes one `locale.translation_fallback` warning per key and language, up
/// to [`MAX_TRANSLATION_FALLBACK_LINES`].
///
/// The registry records each fallback (English text) or key display once per
/// key and language for the whole run, so this reads that session list. A
/// record carries the key and language only, never a parameter value. When the
/// list is longer than the cap, one more line with the same reason key
/// reports how many records were not written (`[omitted=N]`, no key or
/// language). The warning does not change the exit code.
pub(crate) fn write_translation_fallbacks(stderr: &mut impl Write, registry: &Registry) {
    let records = registry.missing_messages();
    let Some(first) = records.first() else {
        return;
    };
    for record in records.iter().take(MAX_TRANSLATION_FALLBACK_LINES) {
        let cause = registry.resolve_plain_text(&record.language, TRANSLATION_FALLBACK_CAUSE_KEY);
        let line = match &cause {
            Some(cause) => format!(
                "DiskUnfold: {TRANSLATION_FALLBACK_REASON_KEY}: {cause} [key={} language={}]",
                record.key, record.language
            ),
            None => format!(
                "DiskUnfold: {TRANSLATION_FALLBACK_REASON_KEY} [key={} language={}]",
                record.key, record.language
            ),
        };
        write_stderr_line(stderr, &line);
    }
    let omitted = records.len().saturating_sub(MAX_TRANSLATION_FALLBACK_LINES);
    if omitted == 0 {
        return;
    }
    // The summary line carries no key or language of its own; the first record picks its text language.
    let summary = registry.resolve_plain_text(&first.language, TRANSLATION_FALLBACK_OMITTED_KEY);
    let line = match &summary {
        Some(summary) => {
            format!("DiskUnfold: {TRANSLATION_FALLBACK_REASON_KEY}: {summary} [omitted={omitted}]")
        }
        None => format!("DiskUnfold: {TRANSLATION_FALLBACK_REASON_KEY} [omitted={omitted}]"),
    };
    write_stderr_line(stderr, &line);
}
