//! GUI history list registration and authenticated session cursors.

pub use du_core_contracts::history_ipc::HistoryListFilter as Filter;
use du_core_contracts::{
    PathBytes,
    history_ipc::{HistoryListRequest, HistoryListResponse, HistoryListRevision, HistoryListRow},
    ports::{PortError, Random},
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

const DOMAIN: &[u8] = b"diskunfold.history.list.h1\0";
const PAYLOAD_LEN: usize = 64;
const MAX_TOKEN: usize = 2048;

/// A history revision pinned to one deletion generation and record upper bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Revision {
    pub delete_generation: u64,
    pub max_record_seq: u64,
}

/// The last row's descending sort key.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Position {
    pub reckon_at_ms: i64,
    pub record_seq: u64,
}

/// A position with either its readable summary or an unavailable row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry<T> {
    pub position: Position,
    /// `None` represents a row-level read failure, not an empty or deleted row.
    pub summary: Option<T>,
}

/// A page produced by the history reader using one consistent snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadPage<T> {
    pub revision: Revision,
    pub entries: Vec<Entry<T>>,
    pub has_more: bool,
}

/// History storage failures distinguished by the IF-07 reader.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadError {
    VersionChanged,
    Unavailable,
}

/// A synchronous read port. An implementation must apply the complete filter,
/// pin the first revision, and reject a changed deletion generation.
pub trait HistoryRead {
    type Summary;
    fn page(
        &self,
        filter: &Filter,
        revision: Option<Revision>,
        after: Option<Position>,
        count: usize,
    ) -> Result<ReadPage<Self::Summary>, ReadError>;
}

/// Failure before any public transport-specific conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListError {
    InvalidCursor,
    InvalidLimit,
    InvalidFilter,
    VersionChanged,
    StorageUnavailable,
}

/// A list page with an authenticated continuation tied to this session.
pub struct ListPage<T> {
    pub revision: Revision,
    pub entries: Vec<Entry<T>>,
    pub next_cursor: Option<String>,
}

/// An immutable reader and its session key. Replacing the key retires cursors.
pub struct HistoryList<R> {
    reader: R,
    key: [u8; 32],
}

/// Registers the local list command using a new random key for this GUI lifetime.
pub fn register<R>(
    builder: &mut EngineBuilder,
    reader: R,
    random: &dyn Random,
) -> Result<(), PortError>
where
    R: HistoryRead<Summary = HistoryListRow> + Send + Sync + 'static,
{
    let mut key = [0; 32];
    random.fill(&mut key)?;
    builder.register(HistoryList::new(reader, key));
    Ok(())
}

impl<R> UseCase for HistoryList<R>
where
    R: HistoryRead<Summary = HistoryListRow> + Send + Sync + 'static,
{
    const NAME: &'static str = "history.list";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = HistoryListRequest;
    type Response = HistoryListResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let page = self
            .list(
                &request.filter.unwrap_or_default(),
                request.cursor.as_deref(),
                request.count,
            )
            .map_err(list_error)?;
        let rows = page
            .entries
            .into_iter()
            .map(|entry| entry.summary.ok_or(ListError::StorageUnavailable))
            .collect::<Result<_, _>>()
            .map_err(list_error)?;
        Ok(HistoryListResponse {
            revision: HistoryListRevision {
                delete_generation: page.revision.delete_generation.to_string(),
                max_record_seq: page.revision.max_record_seq.to_string(),
            },
            rows,
            next_cursor: page.next_cursor,
        })
    }
}

fn list_error(error: ListError) -> EngineError {
    EngineError::reason(match error {
        ListError::InvalidCursor => "cursor.invalid",
        ListError::InvalidLimit => "cursor.limit_exceeded",
        ListError::InvalidFilter => "command.body_invalid",
        ListError::VersionChanged => "history.version_changed",
        ListError::StorageUnavailable => "history.storage_unavailable",
    })
}

impl<R: HistoryRead> HistoryList<R> {
    /// Construct a list boundary with a caller-supplied CSPRNG session key.
    #[must_use]
    pub fn new(reader: R, key: [u8; 32]) -> Self {
        Self { reader, key }
    }

    /// Read a page without exposing the reader's internal continuation.
    ///
    /// # Errors
    /// Invalid filters, limits or cursors are rejected; stale generations and
    /// storage failures are distinguished. An invalid reader page fails closed.
    pub fn list(
        &self,
        filter: &Filter,
        cursor: Option<&str>,
        count: Option<u32>,
    ) -> Result<ListPage<R::Summary>, ListError> {
        let count = count.unwrap_or(200);
        if !(1..=1000).contains(&count) {
            return Err(ListError::InvalidLimit);
        }
        if matches!((filter.finished_from_ms, filter.finished_before_ms), (Some(a), Some(b)) if a >= b)
        {
            return Err(ListError::InvalidFilter);
        }
        let digest = filter_digest(filter)?;
        let prior = cursor
            .map(|token| decode(&self.key, token, digest))
            .transpose()?;
        let page = self
            .reader
            .page(
                filter,
                prior.map(|v| v.0),
                prior.map(|v| v.1),
                count as usize,
            )
            .map_err(|e| match e {
                ReadError::VersionChanged => ListError::VersionChanged,
                ReadError::Unavailable => ListError::StorageUnavailable,
            })?;
        validate_page(&page, prior, count as usize)?;
        let next_cursor = if page.has_more {
            let position = page
                .entries
                .last()
                .ok_or(ListError::StorageUnavailable)?
                .position;
            Some(encode(&self.key, digest, page.revision, position))
        } else {
            None
        };
        Ok(ListPage {
            revision: page.revision,
            entries: page.entries,
            next_cursor,
        })
    }
}

fn validate_page<T>(
    page: &ReadPage<T>,
    prior: Option<(Revision, Position)>,
    limit: usize,
) -> Result<(), ListError> {
    if prior.is_some_and(|(rev, _)| rev.delete_generation != page.revision.delete_generation) {
        return Err(ListError::VersionChanged);
    }
    if prior.is_some_and(|(rev, _)| rev != page.revision)
        || page.entries.len() > limit
        || (page.has_more && page.entries.is_empty())
    {
        return Err(ListError::StorageUnavailable);
    }
    let mut previous = prior.map(|(_, pos)| pos);
    for entry in &page.entries {
        let pos = entry.position;
        if pos.record_seq == 0
            || pos.record_seq > page.revision.max_record_seq
            || previous.is_some_and(|p| pos >= p)
        {
            return Err(ListError::StorageUnavailable);
        }
        previous = Some(pos);
    }
    Ok(())
}

fn filter_digest(filter: &Filter) -> Result<[u8; 32], ListError> {
    let bytes = serde_json::to_vec(filter).map_err(|_| ListError::InvalidFilter)?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

fn encode(key: &[u8; 32], digest: [u8; 32], revision: Revision, pos: Position) -> String {
    let mut bytes = [0; PAYLOAD_LEN];
    bytes[..32].copy_from_slice(&digest);
    bytes[32..40].copy_from_slice(&revision.delete_generation.to_le_bytes());
    bytes[40..48].copy_from_slice(&revision.max_record_seq.to_le_bytes());
    bytes[48..56].copy_from_slice(&pos.reckon_at_ms.to_le_bytes());
    bytes[56..].copy_from_slice(&pos.record_seq.to_le_bytes());
    format!(
        "h1.{}.{}",
        PathBytes::from_bytes(&bytes).as_str(),
        PathBytes::from_bytes(crate::cursor_mac::mac(key, DOMAIN, &bytes).as_bytes()).as_str()
    )
}

fn decode(
    key: &[u8; 32],
    token: &str,
    digest: [u8; 32],
) -> Result<(Revision, Position), ListError> {
    if token.len() > MAX_TOKEN {
        return Err(ListError::InvalidCursor);
    }
    let mut parts = token.split('.');
    if parts.next() != Some("h1") {
        return Err(ListError::InvalidCursor);
    }
    let payload: [u8; PAYLOAD_LEN] = part(parts.next())?;
    let signature: [u8; 32] = part(parts.next())?;
    if parts.next().is_some()
        || crate::cursor_mac::mac(key, DOMAIN, &payload) != signature
        || payload[..32] != digest
    {
        return Err(ListError::InvalidCursor);
    }
    let word = |start| -> Result<[u8; 8], ListError> {
        payload[start..start + 8]
            .try_into()
            .map_err(|_| ListError::InvalidCursor)
    };
    let revision = Revision {
        delete_generation: u64::from_le_bytes(word(32)?),
        max_record_seq: u64::from_le_bytes(word(40)?),
    };
    let position = Position {
        reckon_at_ms: i64::from_le_bytes(word(48)?),
        record_seq: u64::from_le_bytes(word(56)?),
    };
    if position.record_seq == 0 || position.record_seq > revision.max_record_seq {
        return Err(ListError::InvalidCursor);
    }
    Ok((revision, position))
}

fn part<const N: usize>(value: Option<&str>) -> Result<[u8; N], ListError> {
    PathBytes::parse(value.ok_or(ListError::InvalidCursor)?)
        .map_err(|_| ListError::InvalidCursor)?
        .bytes()
        .map_err(|_| ListError::InvalidCursor)?
        .try_into()
        .map_err(|_| ListError::InvalidCursor)
}

#[cfg(test)]
mod tests;
