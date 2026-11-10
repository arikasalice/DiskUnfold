//! A non-releasable free-text check over one fixed secret snapshot.
use crate::fixed_exclusion::{Classified, Intermediate};
use du_core_contracts::share::{FieldClass, SecretCheck, SecretCheckSnapshot};
use std::collections::BTreeMap;

/// F37 limitation key when a free-text field cannot be retained.
pub const SECRET_CHECK_UNAVAILABLE: &str = "share.secret_check_unavailable";

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
/// field and record the published limitation. Non-string free-text fields are
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

#[cfg(test)]
mod tests;
