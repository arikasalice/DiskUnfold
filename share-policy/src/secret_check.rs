//! A non-releasable free-text check over one fixed secret snapshot.
use crate::fixed_exclusion::{Classified, Intermediate};
use du_core_contracts::share::{FieldClass, SecretCheck, SecretCheckSnapshot};
use serde_json::Value;
use std::collections::BTreeMap;

/// F37 limitation key when a free-text field cannot be retained.
pub const SECRET_CHECK_UNAVAILABLE: &str = "share.secret_check_unavailable";
const MAX_CHECK_BYTES: usize = 1024 * 1024;

/// Final secret verification failed without returning any rendered content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderedSecretFound;

/// A JSON rendering failed an intermediate-only comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntermediateRenderError {
    /// The retained admission snapshot could not clear the rendered bytes.
    SecretCheck,
    /// The JSON leaves, values, or supported shape did not match.
    Mismatch,
}

impl RenderedSecretFound {
    /// Stable reason for a rejected rendered secret check.
    #[must_use]
    pub const fn reason_key(self) -> &'static str {
        "share.rendered_secret_found"
    }
}

/// Opaque checked data and the same snapshot, retained for a later final check.
///
/// The snapshot is dropped with this intermediate on completion, failure or
/// cancellation. This is not a complete filter result and cannot be released.
pub struct Checked {
    _fields: BTreeMap<String, Classified>,
    _snapshot: Option<Box<dyn SecretCheckSnapshot>>,
    _fixed_excluded: usize,
    _secret_check_excluded: usize,
}

/// Checks only declared free text with the snapshot opened at export admission.
///
/// The caller must open the snapshot before preparing fields. A missing
/// snapshot or a `Contains` or `Unavailable` answer removes the affected free-text
/// field and records the published limitation. Non-string free-text fields are
/// likewise removed because this stage cannot check them. No path, masking,
/// sealing, rendered-byte verification, or export is performed here.
#[must_use]
pub fn check(input: Intermediate, snapshot: Option<Box<dyn SecretCheckSnapshot>>) -> Checked {
    let mut excluded = 0;
    let mut fields = input.fields;
    fields.retain(|_, field| {
        if field.class != FieldClass::FreeText {
            return true;
        }
        let clean = field.value.as_str().is_some_and(|text| {
            snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.check(text) == SecretCheck::Clean)
        });
        if !clean {
            excluded += 1;
        }
        clean
    });
    Checked {
        _fields: fields,
        _snapshot: snapshot,
        _fixed_excluded: input.excluded,
        _secret_check_excluded: excluded,
    }
}

impl Checked {
    /// Checks secret content and compares flat JSON to the retained intermediate.
    ///
    /// This verifies exact object keys and scalar values, rejecting floats,
    /// nested values and arrays. JSON object key order is not significant.
    /// This intermediate has not passed scope, mode transformation, preview
    /// digest or final policy authorization; even a match permits no release.
    /// A complete filter must build its final allowed paths after those stages.
    ///
    /// # Errors
    /// Returns an error for an unsuccessful secret check or a mismatch.
    pub fn check_intermediate_json(&self, bytes: &[u8]) -> Result<(), IntermediateRenderError> {
        let value = self
            .checked_json(bytes)
            .map_err(|_| IntermediateRenderError::SecretCheck)?;
        let object = value.as_object().ok_or(IntermediateRenderError::Mismatch)?;
        if object.len() != self._fields.len() {
            return Err(IntermediateRenderError::Mismatch);
        }
        for (key, field) in &self._fields {
            let value = object.get(key).ok_or(IntermediateRenderError::Mismatch)?;
            if !supported_scalar(value) || !supported_scalar(&field.value) || *value != field.value
            {
                return Err(IntermediateRenderError::Mismatch);
            }
        }
        Ok(())
    }

    /// Checks a JSON rendering using the same snapshot as the free-text stage.
    ///
    /// The raw UTF-8 JSON and each decoded string and object key must pass.
    /// Invalid UTF-8, malformed JSON, duplicate keys, missing snapshot, a
    /// secret pattern, or a non-clean snapshot response fails closed. Other
    /// render formats are unsupported by this operation. The complete rendering
    /// must fit within the published snapshot input limit of 1 MiB.
    ///
    /// Passing does not prove allowed field paths, scope, policy, digest, or
    /// atomic output and must not be treated as authorization to release.
    ///
    /// # Errors
    /// Returns `RenderedSecretFound` whenever any check cannot pass.
    pub fn check_rendered_json(&self, bytes: &[u8]) -> Result<(), RenderedSecretFound> {
        self.checked_json(bytes).map(drop)
    }

    fn checked_json(&self, bytes: &[u8]) -> Result<Value, RenderedSecretFound> {
        if bytes.len() > MAX_CHECK_BYTES {
            return Err(RenderedSecretFound);
        }
        let snapshot = self._snapshot.as_ref().ok_or(RenderedSecretFound)?;
        let text = std::str::from_utf8(bytes).map_err(|_| RenderedSecretFound)?;
        check_text(snapshot.as_ref(), text)?;
        let value = crate::unique_json::parse(bytes).map_err(|_| RenderedSecretFound)?;
        check_value(snapshot.as_ref(), &value)?;
        Ok(value)
    }
    /// Number of free-text fields removed by the secret check.
    #[must_use]
    pub fn secret_check_excluded(&self) -> usize {
        self._secret_check_excluded
    }

    /// Returns the limitation key if at least one field was removed.
    #[must_use]
    pub fn limitation(&self) -> Option<&'static str> {
        (self._secret_check_excluded > 0).then_some(SECRET_CHECK_UNAVAILABLE)
    }
}

fn supported_scalar(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => true,
        Value::Number(number) => number.is_i64() || number.is_u64(),
        Value::Array(_) | Value::Object(_) => false,
    }
}

fn check_text(snapshot: &dyn SecretCheckSnapshot, text: &str) -> Result<(), RenderedSecretFound> {
    if crate::fixed_exclusion::secret_pattern(text) || snapshot.check(text) != SecretCheck::Clean {
        return Err(RenderedSecretFound);
    }
    Ok(())
}

fn check_value(
    snapshot: &dyn SecretCheckSnapshot,
    value: &Value,
) -> Result<(), RenderedSecretFound> {
    match value {
        Value::String(text) => check_text(snapshot, text),
        Value::Array(values) => values
            .iter()
            .try_for_each(|value| check_value(snapshot, value)),
        Value::Object(values) => values.iter().try_for_each(|(key, value)| {
            check_text(snapshot, key)?;
            check_value(snapshot, value)
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests;
