//! Validation of stable source metadata, independent of snapshot publication.
use crate::records::NameEncoding;
use serde_json::Value;
use std::collections::HashSet;

pub(super) fn validate(json: &Value, encoding: NameEncoding, root_count: usize) -> Result<(), ()> {
    let source = json.get("source").and_then(Value::as_object).ok_or(())?;
    let marker = source.get("scopeMarker").ok_or(())?;
    if !marker.is_null()
        && !marker.as_str().is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    {
        return Err(());
    }
    if !matches!(source.get("completeness"), Some(Value::Null))
        && !matches!(
            source.get("completeness").and_then(Value::as_str),
            Some("complete" | "partial" | "missing")
        )
    {
        return Err(());
    }
    let rules = source
        .get("pathRules")
        .and_then(Value::as_array)
        .ok_or(())?;
    if rules.len() != root_count {
        return Err(());
    }
    let separator = match encoding {
        NameEncoding::Bytes => "/",
        NameEncoding::WindowsUtf16Le => "\\",
    };
    for rule in rules {
        let rule = rule.as_object().ok_or(())?;
        if rule.get("separator").and_then(Value::as_str) != Some(separator)
            || !enum_or_null(rule.get("caseSensitivity"), &["sensitive", "insensitive"])
            || !enum_or_null(rule.get("normalization"), &["none", "NFC", "NFD"])
        {
            return Err(());
        }
    }
    let _ = entries(source.get("conditionSummary"))?;
    if let Some(values) = entries(source.get("algorithms"))? {
        let mut seen = HashSet::new();
        for value in values {
            let value = value.as_object().ok_or(())?;
            let kind = value
                .get("kind")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or(())?;
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or(())?;
            let version = value
                .get("version")
                .and_then(Value::as_u64)
                .filter(|n| (1..=u64::from(u32::MAX)).contains(n))
                .ok_or(())?;
            if !seen.insert((kind, id, version)) {
                return Err(());
            }
        }
    }
    Ok(())
}

fn enum_or_null(value: Option<&Value>, options: &[&str]) -> bool {
    value.is_some_and(|v| v.is_null() || v.as_str().is_some_and(|s| options.contains(&s)))
}

fn entries(value: Option<&Value>) -> Result<Option<&Vec<Value>>, ()> {
    let object = value.and_then(Value::as_object).ok_or(())?;
    match object.get("state").and_then(Value::as_str) {
        Some("known") => Ok(Some(
            object.get("entries").and_then(Value::as_array).ok_or(())?,
        )),
        Some("unknown") if !object.contains_key("entries") => Ok(None),
        _ => Err(()),
    }
}
