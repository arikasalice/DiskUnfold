//! GUI-only history item paging with a session-authenticated continuation.

use du_core_contracts::{
    Id, PathBytes,
    history_ipc::{
        HistoryDetailCompleteness, HistoryItemCounts, HistoryItemOutcome, HistoryItemResult,
        HistoryItemRow, HistoryItemsRequest, HistoryItemsResponse, HistoryListRevision,
    },
    ports::{PortError, Random},
};

use super::{MAX_TOKEN, Revision, part};
use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    cursor_mac::mac,
    error::EngineError,
};

const DOMAIN: &[u8] = b"diskunfold.history.items.hi1\0";

/// The last position and inclusive upper bound of an authenticated item page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub revision: Revision,
    pub last: u64,
    pub upper: u64,
}

/// The item results and reconciliation inputs from one SQLite snapshot.
pub struct Page {
    pub revision: Revision,
    pub rows: Vec<(u64, Option<HistoryItemResult>)>,
    pub next: Option<Cursor>,
    pub summary: Option<u128>,
    pub recorded: u64,
    pub item_history_expected: bool,
}

/// Errors of the read operation as a whole.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadError {
    Removed,
    VersionChanged,
    StorageUnavailable,
}

/// Loads one operation's immutable item results from a pinned history revision.
pub trait Read {
    fn page(
        &self,
        job: &Id,
        outcome: Option<HistoryItemOutcome>,
        cursor: Option<Cursor>,
        count: usize,
    ) -> Result<Page, ReadError>;
}

/// Registers history.items on the GUI channel with an independent session key.
pub fn register<R: Read + Send + Sync + 'static>(
    builder: &mut EngineBuilder,
    reader: R,
    random: &dyn Random,
) -> Result<(), PortError> {
    let mut key = [0; 32];
    random.fill(&mut key)?;
    builder.register(Items { reader, key });
    Ok(())
}

struct Items<R> {
    reader: R,
    key: [u8; 32],
}

impl<R: Read + Send + Sync + 'static> UseCase for Items<R> {
    const NAME: &'static str = "history.items";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = HistoryItemsRequest;
    type Response = HistoryItemsResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let count = request.count.unwrap_or(200);
        if !(1..=1000).contains(&count) {
            return Err(EngineError::reason("cursor.limit_exceeded"));
        }
        let digest = scope_digest(&request.job_ref, request.outcome);
        let cursor = request
            .cursor
            .as_deref()
            .map(|token| {
                decode(&self.key, token, &digest)
                    .ok_or_else(|| EngineError::reason("cursor.invalid"))
            })
            .transpose()?;
        let page = self
            .reader
            .page(&request.job_ref, request.outcome, cursor, count as usize)
            .map_err(|error| {
                EngineError::reason(match error {
                    ReadError::Removed => "history.removed",
                    ReadError::VersionChanged => "history.version_changed",
                    ReadError::StorageUnavailable => "history.storage_unavailable",
                })
            })?;
        validate(&page, cursor, count as usize)?;
        let next = page.next.map(|next| encode(&self.key, digest, next));
        Ok(project(page, next))
    }
}

fn validate(page: &Page, prior: Option<Cursor>, count: usize) -> Result<(), EngineError> {
    if prior
        .is_some_and(|cursor| cursor.revision.delete_generation != page.revision.delete_generation)
    {
        return Err(EngineError::reason("history.version_changed"));
    }
    let invalid = || EngineError::reason("history.storage_unavailable");
    if prior.is_some_and(|cursor| cursor.revision != page.revision)
        || page.rows.len() > count
        || page.rows.len() as u64 > page.recorded
    {
        return Err(invalid());
    }
    let mut last = prior.map(|cursor| cursor.last);
    for &(sequence, _) in &page.rows {
        if last.is_some_and(|previous| sequence <= previous)
            || prior.is_some_and(|cursor| sequence > cursor.upper)
        {
            return Err(invalid());
        }
        last = Some(sequence);
    }
    if let Some(next) = page.next
        && (page.rows.is_empty()
            || next.revision != page.revision
            || last != Some(next.last)
            || next.last >= next.upper
            || prior.is_some_and(|cursor| cursor.upper != next.upper))
    {
        return Err(invalid());
    }
    Ok(())
}

fn project(page: Page, next_cursor: Option<String>) -> HistoryItemsResponse {
    let difference = page
        .summary
        .map(|summary| summary.abs_diff(u128::from(page.recorded)));
    let mut missing_reasons = Vec::new();
    if page.rows.iter().any(|(_, row)| row.is_none()) {
        missing_reasons.push("read_failed".into());
    }
    if page.item_history_expected && difference != Some(0) {
        missing_reasons.push("items_incomplete".into());
    }
    HistoryItemsResponse {
        revision: HistoryListRevision {
            delete_generation: page.revision.delete_generation.to_string(),
            max_record_seq: page.revision.max_record_seq.to_string(),
        },
        rows: page
            .rows
            .into_iter()
            .map(|(seq, result)| HistoryItemRow {
                item_seq: seq.to_string(),
                result,
            })
            .collect(),
        counts: HistoryItemCounts {
            summary: page.summary.map(|value| value.to_string()),
            recorded: page.recorded.to_string(),
            unavailable: difference.map(|value| value.to_string()),
        },
        completeness: if missing_reasons.is_empty() {
            HistoryDetailCompleteness::Complete
        } else {
            HistoryDetailCompleteness::Partial
        },
        missing_reasons,
        next_cursor,
    }
}

fn scope_digest(job: &Id, outcome: Option<HistoryItemOutcome>) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(job.as_str().as_bytes());
    if let Some(outcome) = outcome {
        hasher.update(outcome.as_str().as_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn encode(key: &[u8; 32], scope: [u8; 32], cursor: Cursor) -> String {
    let mut payload = [0; 64];
    payload[..32].copy_from_slice(&scope);
    let (words, _) = payload[32..].as_chunks_mut::<8>();
    for (word, value) in words.iter_mut().zip([
        cursor.revision.delete_generation,
        cursor.revision.max_record_seq,
        cursor.last,
        cursor.upper,
    ]) {
        word.copy_from_slice(&value.to_le_bytes());
    }
    format!(
        "hi1.{}.{}",
        PathBytes::from_bytes(&payload).as_str(),
        PathBytes::from_bytes(mac(key, DOMAIN, &payload).as_bytes()).as_str(),
    )
}

fn decode(key: &[u8; 32], token: &str, scope: &[u8; 32]) -> Option<Cursor> {
    if token.len() > MAX_TOKEN {
        return None;
    }
    let mut fields = token.split('.');
    if fields.next()? != "hi1" {
        return None;
    }
    let payload: [u8; 64] = part(fields.next()).ok()?;
    let signature: [u8; 32] = part(fields.next()).ok()?;
    if fields.next().is_some() || mac(key, DOMAIN, &payload) != signature || &payload[..32] != scope
    {
        return None;
    }
    let word = |i| Some(u64::from_le_bytes(payload.get(i..i + 8)?.try_into().ok()?));
    let cursor = Cursor {
        revision: Revision {
            delete_generation: word(32)?,
            max_record_seq: word(40)?,
        },
        last: word(48)?,
        upper: word(56)?,
    };
    (cursor.last < cursor.upper).then_some(cursor)
}

#[cfg(test)]
mod tests;
