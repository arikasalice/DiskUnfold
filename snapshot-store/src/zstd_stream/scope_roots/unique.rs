//! Reject duplicate JSON object keys after serde_json has validated the document.
use std::collections::HashSet;

/// Examine only structural tokens; serde_json validates syntax and decodes keys.
pub(super) fn validate(json: &str) -> Result<(), ()> {
    let bytes = json.as_bytes();
    let mut containers: Vec<Option<HashSet<String>>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                containers.push(Some(HashSet::new()));
                i += 1;
            }
            b'[' => {
                containers.push(None);
                i += 1;
            }
            b'}' | b']' => {
                containers.pop().ok_or(())?;
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i >= bytes.len() {
                    return Err(());
                }
                i += 1;
                let mut next = i;
                while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                    next += 1;
                }
                if bytes.get(next) == Some(&b':') {
                    let key: String = serde_json::from_str(&json[start..i]).map_err(|_| ())?;
                    let keys = containers.last_mut().and_then(Option::as_mut).ok_or(())?;
                    if !keys.insert(key) {
                        return Err(());
                    }
                }
            }
            _ => i += 1,
        }
    }
    if containers.is_empty() {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_decoded_keys_per_object_without_confusing_string_content() {
        for json in [r#"{"a":1,"\u0061":2}"#, r#"{"x":{"k":1,"k":2}}"#] {
            assert!(serde_json::from_str::<serde_json::Value>(json).is_ok());
            assert!(validate(json).is_err());
        }
        for json in [
            r#"{"a":{"x":1},"b":{"x":2}}"#,
            r#"{"value":"braces { [ \" , colon :","other":[{"v":1}]}"#,
        ] {
            assert!(serde_json::from_str::<serde_json::Value>(json).is_ok());
            assert!(validate(json).is_ok());
        }
    }
}
