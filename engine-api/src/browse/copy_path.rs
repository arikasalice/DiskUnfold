//! Prepare one complete clipboard payload from immutable observed paths.
use du_analysis_index::{IngestSink, QueryError};
use du_core_contracts::{
    browse_ipc::{BrowseCopyPathRefusal, BrowseCopyPathRequest},
    result::ResultNativePath,
};

use crate::error::EngineError;

/// Reads exactly the requested revision from the desktop's result store.
pub struct PathCopy(pub(crate) IngestSink);

/// The native payload is for the clipboard adapter, never an IPC response.
pub struct PreparedPathCopy {
    pub text: String,
    pub count: u32,
}

/// Checks the bounded request before inspecting stored observations.
fn validate(request: &BrowseCopyPathRequest) -> Result<(), EngineError> {
    if !(1..=1_000).contains(&request.items.len()) {
        return Err(EngineError::reason("command.body_invalid"));
    }
    if request
        .items
        .iter()
        .any(|item| item.result_version != request.result_version)
    {
        return Err(EngineError::reason("result.requery_required"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

impl PathCopy {
    /// Pins the revision and collects observed paths in the result's item order.
    ///
    /// The adapter supplies exact native text conversion. A single unsupported
    /// path or ambiguous multi-line path refuses the complete batch.
    pub fn prepare(
        &self,
        mut request: BrowseCopyPathRequest,
        line_ending: &'static str,
        mut path_text: impl FnMut(&ResultNativePath) -> Option<String>,
    ) -> Result<Result<PreparedPathCopy, BrowseCopyPathRefusal>, EngineError> {
        validate(&request)?;
        let version = self
            .0
            .pin_current(&request.result_version)
            .map_err(super::reason)?;
        request.items.sort_unstable_by_key(|item| item.item_no);
        // Establish every reference before constructing a platform payload.
        for item in &request.items {
            version.detail(item).map_err(super::reason)?;
        }
        let multiline_check = request.items.len() > 1;
        let mut multiline = false;
        let mut text = String::new();
        for (index, item) in request.items.iter().enumerate() {
            let path = match version.native_path(item) {
                Ok(path) => path,
                Err(QueryError::InvalidPath | QueryError::PathUnavailable) => {
                    return Ok(Err(BrowseCopyPathRefusal::Unrepresentable));
                }
                Err(error) => return Err(super::reason(error)),
            };
            let Some(path) = path_text(&path) else {
                return Ok(Err(BrowseCopyPathRefusal::Unrepresentable));
            };
            multiline |=
                multiline_check && path.contains(['\r', '\n', '\u{85}', '\u{2028}', '\u{2029}']);
            if index != 0 {
                text.push_str(line_ending);
            }
            text.push_str(&path);
        }
        if multiline {
            return Ok(Err(BrowseCopyPathRefusal::Multiline));
        }
        let count = u32::try_from(request.items.len())
            .map_err(|_| EngineError::reason("command.body_invalid"))?;
        Ok(Ok(PreparedPathCopy { text, count }))
    }
}
