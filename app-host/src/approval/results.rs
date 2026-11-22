//! GUI-only paging over F11 results retained in the current app session.
//!
//! Design items: F11 FR10.HD1.app-host.DD1, F11 FR6.HD2.app-host.DD1,
//! F21 FR3.HD1.

use super::*;
use du_change_executor::trash::ChangeResultItem;
use du_core_contracts::{
    PathBytes,
    change_ipc::{
        CHANGE_ITEM_RESULTS, ChangeItemResultFilter, ChangeItemResultsRequest,
        ChangeItemResultsResponse, ChangeItemState,
    },
};
use du_engine_api::error::EngineError;
use sha2::{Digest, Sha256};

const PAGE_SIZE: usize = 100;
const MAX_CURSOR_BYTES: usize = 512;
const CURSOR_DOMAIN: &[u8] = b"diskunfold.change.item_results.ci1\0";

pub(super) struct ChangeItemResults(pub(super) Arc<Runtime>);

impl UseCase for ChangeItemResults {
    const NAME: &'static str = CHANGE_ITEM_RESULTS;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ChangeItemResultsRequest;
    type Response = ChangeItemResultsResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let change = self
            .0
            .change
            .as_ref()
            .ok_or_else(|| reason("change.result_not_found"))?;
        let results = change
            .executions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&request.job_ref)
            .map(|entry| entry.results.clone())
            .ok_or_else(|| reason("job.not_found"))?;
        let job = change
            .jobs
            .snapshot(&request.job_ref)
            .map_err(|_| reason("job.not_found"))?;
        let cursor_key = *change
            .cursor_key
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let offset = request
            .cursor
            .as_deref()
            .map(|token| decode_cursor(&cursor_key, &request.job_ref, request.result_filter, token))
            .transpose()?
            .unwrap_or(0);

        let snapshot = results.snapshot();
        let terminal = job.state.is_terminal();
        let mut items = snapshot
            .items
            .into_iter()
            .filter(|item| terminal || item.result.state != ChangeItemState::Unprocessed)
            .filter(|item| matches_filter(item, request.result_filter))
            .collect::<Vec<_>>();
        if offset > items.len() {
            return Err(reason("cursor.invalid"));
        }
        let end = offset.saturating_add(PAGE_SIZE).min(items.len());
        let next_cursor = (end < items.len())
            .then(|| encode_cursor(&cursor_key, &request.job_ref, request.result_filter, end));
        let mut page = items.drain(offset..end).collect::<Vec<_>>();
        for item in &mut page {
            item.result.undo_available = change
                .undo
                .contains(&request.job_ref, item.item_no)
                .unwrap_or(false);
        }

        Ok(ChangeItemResultsResponse {
            job_ref: request.job_ref,
            job_state: job.state,
            counts: snapshot.counts,
            item_recording: snapshot.item_recording,
            items: page.into_iter().map(project_item).collect(),
            next_cursor,
        })
    }
}

fn project_item(item: ChangeResultItem) -> du_core_contracts::change_ipc::ChangeItemResult {
    item.result
}

fn matches_filter(item: &ChangeResultItem, filter: Option<ChangeItemResultFilter>) -> bool {
    match filter {
        None => true,
        Some(ChangeItemResultFilter::Completed) => item.result.state == ChangeItemState::Completed,
        Some(ChangeItemResultFilter::Failed) => item.result.state == ChangeItemState::Failed,
        Some(ChangeItemResultFilter::OutcomeUnknown) => {
            item.result.state == ChangeItemState::Failed && item.result.outcome_unknown
        }
        Some(ChangeItemResultFilter::Skipped) => item.result.state == ChangeItemState::Skipped,
        Some(ChangeItemResultFilter::Held) => {
            item.result.state == ChangeItemState::Skipped && item.result.held
        }
        Some(ChangeItemResultFilter::Unprocessed) => {
            item.result.state == ChangeItemState::Unprocessed
        }
    }
}

fn encode_cursor(
    key: &[u8; 32],
    job_ref: &Id,
    filter: Option<ChangeItemResultFilter>,
    offset: usize,
) -> String {
    let mut payload = Vec::with_capacity(45);
    payload.extend_from_slice(job_ref.as_str().as_bytes());
    payload.push(filter_code(filter));
    payload.extend_from_slice(&(offset as u64).to_le_bytes());
    let signature = hmac(key, CURSOR_DOMAIN, &payload);
    format!(
        "ci1.{}.{}",
        PathBytes::from_bytes(&payload).as_str(),
        PathBytes::from_bytes(&signature).as_str(),
    )
}

fn decode_cursor(
    key: &[u8; 32],
    job_ref: &Id,
    filter: Option<ChangeItemResultFilter>,
    token: &str,
) -> Result<usize, EngineError> {
    let invalid = || reason("cursor.invalid");
    if token.len() > MAX_CURSOR_BYTES {
        return Err(invalid());
    }
    let mut parts = token.split('.');
    if parts.next() != Some("ci1") {
        return Err(invalid());
    }
    let payload = PathBytes::parse(parts.next().ok_or_else(invalid)?)
        .and_then(|value| value.bytes().map_err(|error| error.to_string()))
        .map_err(|_| invalid())?;
    let signature = PathBytes::parse(parts.next().ok_or_else(invalid)?)
        .and_then(|value| value.bytes().map_err(|error| error.to_string()))
        .map_err(|_| invalid())?;
    if parts.next().is_some()
        || payload.len() != 45
        || signature.len() != 32
        || payload.get(..36) != Some(job_ref.as_str().as_bytes())
        || payload.get(36) != Some(&filter_code(filter))
        || !constant_time_equal(&signature, &hmac(key, CURSOR_DOMAIN, &payload))
    {
        return Err(invalid());
    }
    let offset = u64::from_le_bytes(payload[37..45].try_into().map_err(|_| invalid())?);
    usize::try_from(offset).map_err(|_| invalid())
}

fn reason(key: &'static str) -> EngineError {
    EngineError::new(key, Vec::new())
}

fn filter_code(filter: Option<ChangeItemResultFilter>) -> u8 {
    match filter {
        None => 0,
        Some(ChangeItemResultFilter::Completed) => 1,
        Some(ChangeItemResultFilter::Failed) => 2,
        Some(ChangeItemResultFilter::OutcomeUnknown) => 3,
        Some(ChangeItemResultFilter::Skipped) => 4,
        Some(ChangeItemResultFilter::Held) => 5,
        Some(ChangeItemResultFilter::Unprocessed) => 6,
    }
}

fn hmac(key: &[u8; 32], domain: &[u8], payload: &[u8]) -> [u8; 32] {
    let mut inner_pad = [0x36; 64];
    let mut outer_pad = [0x5c; 64];
    for (index, byte) in key.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(domain);
    inner.update(payload);
    let digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(digest);
    outer.finalize().into()
}

fn constant_time_equal(left: &[u8], right: &[u8; 32]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
}
