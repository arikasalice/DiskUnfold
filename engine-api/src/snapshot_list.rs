//! Read a bounded, sequenced page of active snapshot metadata for the GUI.

#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};

use base64::{Engine as Base64Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use du_core_contracts::{
    Id, Timestamp,
    snapshot_index::{
        SnapshotIndexCompleteness, SnapshotIndexCursor, SnapshotIndexEntry, SnapshotIndexError,
        SnapshotIndexPort, SnapshotIndexSourceKind,
    },
    snapshot_list_ipc::{
        SnapshotListCompleteness, SnapshotListRequest, SnapshotListResponse,
        SnapshotListSourceKind, SnapshotListVolume, SnapshotSummary,
    },
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    desktop::DesktopServices,
    error::EngineError,
};

const DEFAULT_LIMIT: i64 = 200;
const MAX_LIMIT: i64 = 1_000;
const MAX_CURSOR_CHARS: usize = 256;

/// Registers the GUI-only snapshot list command over the shared snapshot index.
impl DesktopServices {
    /// Registers an active snapshot list with the host-selected window scope.
    pub fn register_snapshot_list<I>(
        &self,
        builder: &mut EngineBuilder,
        index: Arc<Mutex<I>>,
        primary_window: bool,
    ) where
        I: SnapshotIndexPort + Send + 'static,
    {
        builder.register(ListCommand {
            index,
            primary_window,
        });
    }
}

struct ListCommand<I> {
    index: Arc<Mutex<I>>,
    primary_window: bool,
}

impl<I> UseCase for ListCommand<I>
where
    I: SnapshotIndexPort + Send + 'static,
{
    const NAME: &'static str = "snapshot.list";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SnapshotListRequest;
    type Response = SnapshotListResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        if self.primary_window {
            Ok(())
        } else {
            Err(EngineError::reason("command.window_denied"))
        }
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let limit = page_limit(request.limit)?;
        let cursor = request.cursor.as_deref().map(decode_cursor).transpose()?;
        let page = self
            .index
            .lock()
            .map_err(|_| EngineError::reason("snapshot.store_unavailable"))?
            .list_active(limit, cursor.as_ref())
            .map_err(index_error)?;
        let next_cursor = page.next_cursor.as_ref().map(encode_cursor).transpose()?;
        let items = page
            .rows
            .iter()
            .map(summary)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SnapshotListResponse {
            list_seq: page.list_seq,
            items,
            next_cursor,
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CursorPayload {
    list_seq: i64,
    observed_at_ms: Option<i64>,
    snapshot_id: Id,
}

fn page_limit(limit: Option<i64>) -> Result<usize, EngineError> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    if limit <= 0 {
        return Err(EngineError::reason("snapshot.list_limit_invalid"));
    }
    Ok(limit.min(MAX_LIMIT) as usize)
}

fn decode_cursor(value: &str) -> Result<SnapshotIndexCursor, EngineError> {
    if value.is_empty() || value.len() > MAX_CURSOR_CHARS {
        return Err(EngineError::reason("snapshot.cursor_invalid"));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| EngineError::reason("snapshot.cursor_invalid"))?;
    if URL_SAFE_NO_PAD.encode(&bytes) != value {
        return Err(EngineError::reason("snapshot.cursor_invalid"));
    }
    let cursor: CursorPayload = serde_json::from_slice(&bytes)
        .map_err(|_| EngineError::reason("snapshot.cursor_invalid"))?;
    if cursor.list_seq < 0
        || cursor
            .observed_at_ms
            .is_some_and(|time| Timestamp::from_unix_millis(time).is_none())
    {
        return Err(EngineError::reason("snapshot.cursor_invalid"));
    }
    Ok(SnapshotIndexCursor {
        list_seq: cursor.list_seq,
        observed_at_ms: cursor.observed_at_ms,
        snapshot_id: cursor.snapshot_id,
    })
}

fn encode_cursor(cursor: &SnapshotIndexCursor) -> Result<String, EngineError> {
    let payload = CursorPayload {
        list_seq: cursor.list_seq,
        observed_at_ms: cursor.observed_at_ms,
        snapshot_id: cursor.snapshot_id.clone(),
    };
    let bytes =
        serde_json::to_vec(&payload).map_err(|_| EngineError::reason("support.internal_error"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn summary(entry: &SnapshotIndexEntry) -> Result<SnapshotSummary, EngineError> {
    let row = &entry.row;
    let scope_label = match row.scope_label.as_slice() {
        [] => None,
        bytes if bytes.len() == 32 => Some(hex(bytes)),
        _ => return Err(EngineError::reason("snapshot.store_unavailable")),
    };
    let observed_at = row
        .observed_at_ms
        .map(|millis| {
            Timestamp::from_unix_millis(millis)
                .ok_or_else(|| EngineError::reason("snapshot.store_unavailable"))
        })
        .transpose()?;
    Ok(SnapshotSummary {
        snapshot_id: row.snapshot_id.clone(),
        observed_at,
        scope_label,
        condition_summary: row.condition_summary.clone(),
        completeness: row.completeness.map(|value| match value {
            SnapshotIndexCompleteness::Complete => SnapshotListCompleteness::Complete,
            SnapshotIndexCompleteness::Partial => SnapshotListCompleteness::Partial,
            SnapshotIndexCompleteness::Missing => SnapshotListCompleteness::Missing,
        }),
        format_version: row.format_version,
        body_size: row.body_size,
        pinned: entry.pinned,
        source_kind: match row.source_kind {
            SnapshotIndexSourceKind::Local => SnapshotListSourceKind::Local,
            SnapshotIndexSourceKind::Imported => SnapshotListSourceKind::Imported,
        },
        volumes: entry
            .volumes
            .iter()
            .map(|volume| SnapshotListVolume {
                volume_index: volume.volume_index,
                observed_volume_id: volume.observed_volume_id.as_deref().map(hex),
            })
            .collect(),
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}

fn index_error(error: SnapshotIndexError) -> EngineError {
    match error {
        SnapshotIndexError::StaleCursor => EngineError::reason("snapshot.cursor_stale"),
        SnapshotIndexError::Busy => EngineError::reason("db.busy"),
        _ => EngineError::reason("snapshot.store_unavailable"),
    }
}
