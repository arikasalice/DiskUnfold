//! The outbound filter of the command-line channel (#222).
//!
//! The engine refuses a CLI call without an outbound filter. Until the sharing
//! policy supplies the channel's own filter (F37), the command line admits only
//! the calls the scan command makes and keeps only the fields it uses, so no
//! path or name leaves the engine on this channel.
use du_engine_api::{
    api::{CallerCtx, OutboundFilter},
    error::EngineError,
};
use serde_json::{Map, Value};

pub(crate) struct CliOutbound;

/// The fields each admitted call returns to the command line.
fn kept(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "scope.review" => &["reviewFingerprint", "requestSeq", "notices", "targets"],
        "scan.start" => &[
            "jobRef",
            "existing",
            "scopeVersion",
            "scopeFingerprint",
            "lineageId",
        ],
        "job.status" => &[
            "jobRef",
            "kind",
            "state",
            "sequence",
            "resultVersion",
            "reasonKey",
            "terminal",
        ],
        "job.cancel" => &["outcome", "state", "reasonKey", "sequence"],
        _ => return None,
    })
}

impl OutboundFilter for CliOutbound {
    fn filter(&self, name: &str, _: &CallerCtx, response: Value) -> Result<Value, EngineError> {
        let (Some(fields), Value::Object(mut object)) = (kept(name), response) else {
            return Err(EngineError::new("command.not_provided", Vec::new()));
        };
        let mut kept_object = Map::new();
        for field in fields {
            if let Some(value) = object.remove(*field) {
                kept_object.insert((*field).to_owned(), value);
            }
        }
        // A review target names its path; only its access and reason go out.
        if let Some(Value::Array(targets)) = kept_object.get_mut("targets") {
            for target in targets {
                if let Value::Object(target) = target {
                    target.retain(|key, _| key == "access" || key == "reasonKey");
                }
            }
        }
        Ok(Value::Object(kept_object))
    }
}

#[cfg(test)]
mod tests;
