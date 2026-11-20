//! The outbound filter of the command-line channel (#222).
//!
//! The engine refuses a CLI call without an outbound filter. Until the sharing
//! policy supplies the channel's own filter (F37), the command line admits only
//! the calls the scan command makes and keeps only the fields it uses, so no
//! path or name leaves the engine on this channel.
//!
//! Temporary: when F37 provides the filter of channel `cli`, delete
//! `CliOutbound`, move this field table to share-policy, and register that
//! filter here instead. Errors do not pass through any outbound filter: an
//! error envelope carries only keys and opaque item references.
use du_engine_api::{
    api::{CallerCtx, OutboundFilter},
    error::EngineError,
};
use serde_json::{Map, Value};

pub(crate) struct CliOutbound;

/// The fields each admitted call returns to the command line: exactly the ones
/// `scan_command` reads. Review notices are not sent; the command does not show
/// them.
fn kept(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "scope.review" => &["reviewFingerprint", "targets"],
        "scan.start" => &["jobRef", "existing"],
        "job.status" => &["jobRef", "state", "resultVersion", "terminal"],
        "job.cancel" => &["outcome"],
        _ => return None,
    })
}

/// The fields kept inside each element of a nested object or list.
fn kept_inside(field: &str) -> Option<&'static [&'static str]> {
    Some(match field {
        // A review target names its path; only its access and reason go out.
        "targets" => &["access", "reasonKey"],
        "terminal" => &["completed", "failed", "skipped"],
        _ => return None,
    })
}

fn keep(value: Value, fields: &[&str]) -> Value {
    // Anything but an object has no field to keep, so none of it goes out.
    let Value::Object(mut object) = value else {
        return Value::Null;
    };
    let mut kept = Map::new();
    for field in fields {
        if let Some(value) = object.remove(*field) {
            let value = match (kept_inside(field), value) {
                (Some(inner), Value::Array(items)) => {
                    Value::Array(items.into_iter().map(|item| keep(item, inner)).collect())
                }
                (Some(inner), value) => keep(value, inner),
                (None, value) => value,
            };
            kept.insert((*field).to_owned(), value);
        }
    }
    Value::Object(kept)
}

impl OutboundFilter for CliOutbound {
    fn filter(&self, name: &str, _: &CallerCtx, response: Value) -> Result<Value, EngineError> {
        let (Some(fields), Value::Object(_)) = (kept(name), &response) else {
            return Err(EngineError::new("command.not_provided", Vec::new()));
        };
        Ok(keep(response, fields))
    }
}

#[cfg(test)]
mod tests;
