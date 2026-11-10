//! Initial classification and fixed exclusion, before any release decision.
use du_core_contracts::share::{Exportable, FieldClass};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Invalid input is rejected without returning candidate values or paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Serialization,
    InvalidJson,
    DeclarationMismatch,
    UnsupportedShape,
}

/// Opaque intermediate state, not filtered output and not authority to release.
///
/// Scope, mode transformation, free-text secret checks, sealing and rendered
/// output checks must still run; no accessor exposes the retained data.
pub struct Intermediate {
    pub(crate) fields: BTreeMap<String, Classified>,
    pub(crate) excluded: usize,
}

pub(crate) struct Classified {
    pub(crate) class: FieldClass,
    pub(crate) value: Value,
}

/// Serializes once and applies the first two F37 filter stages.
///
/// This narrow stage supports flat JSON objects with scalar leaves only.
/// It rejects unsupported shapes rather than inventing array declarations.
///
/// # Errors
/// Returns an error for failed serialization, malformed JSON, mismatched or
/// duplicated declarations, or an unsupported shape.
pub fn begin<T: Exportable + Serialize>(value: &T) -> Result<Intermediate, Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Serialization)?;
    begin_json::<T>(&bytes)
}

/// Applies the same validation to serialized candidate bytes.
///
/// This function never releases bytes or treats a matching declaration as
/// evidence of scope, confirmed policy, or a completed secret check.
///
/// # Errors
/// Returns an error for malformed JSON, mismatched declarations, or shape.
pub fn begin_json<T: Exportable>(bytes: &[u8]) -> Result<Intermediate, Error> {
    let tree = crate::unique_json::parse(bytes).map_err(|_| Error::InvalidJson)?;
    let object = tree.as_object().ok_or(Error::UnsupportedShape)?;
    if object.values().any(|v| v.is_array() || v.is_object()) {
        return Err(Error::UnsupportedShape);
    }
    let mut declared = BTreeMap::new();
    for field in T::FIELDS {
        if declared.insert(field.path, field.class).is_some() {
            return Err(Error::DeclarationMismatch);
        }
    }
    let mut retained = BTreeMap::new();
    let mut excluded = 0;
    for (key, value) in object {
        let path = format!("/{}", key.replace('~', "~0").replace('/', "~1"));
        let Some(class) = declared.remove(path.as_str()) else {
            return Err(Error::DeclarationMismatch);
        };
        if matches!(class, FieldClass::Secret | FieldClass::FileContent)
            || secret_name(key)
            || value.as_str().is_some_and(secret_pattern)
        {
            excluded += 1;
        } else {
            retained.insert(
                key.clone(),
                Classified {
                    class,
                    value: value.clone(),
                },
            );
        }
    }
    if !declared.is_empty() {
        return Err(Error::DeclarationMismatch);
    }
    Ok(Intermediate {
        fields: retained,
        excluded,
    })
}

fn secret_name(key: &str) -> bool {
    // This conservative list is not a substitute for the later secret port.
    [
        "connection_key",
        "verifier",
        "credential",
        "authorization",
        "preview_password",
        "approval_token",
    ]
    .iter()
    .any(|name| key.eq_ignore_ascii_case(name))
}

pub(crate) fn secret_pattern(value: &str) -> bool {
    value
        .as_bytes()
        .windows(7)
        .any(|part| part.eq_ignore_ascii_case(b"du_mcp_") || part.eq_ignore_ascii_case(b"bearer "))
}

#[cfg(test)]
mod tests;
