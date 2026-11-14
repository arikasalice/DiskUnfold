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

/// Writes one `locale.translation_fallback` warning per key and language.
///
/// The registry records each fallback (English text) or key display once per
/// key and language for the whole run, so this reads that session list. A
/// record carries the key and language only, never a parameter value. The
/// warning does not change the exit code.
pub(crate) fn write_translation_fallbacks(stderr: &mut impl Write, registry: &Registry) {
    let records = registry.missing_messages();
    if records.is_empty() {
        return;
    }
    for record in records {
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
}
