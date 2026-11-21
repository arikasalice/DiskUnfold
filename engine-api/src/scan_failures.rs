//! GUI-only paging over one immutable scan result.
mod cursor;

use du_analysis_index::{IngestSink, QueryError, ResultVersion};
use du_core_contracts::{
    PathBytes,
    ports::Random,
    result::ResultItemKind,
    scan_failures::{
        ScanFailurePathRef, ScanFailureRow, ScanFailures, ScanFailuresRequest, ScanIssue,
    },
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    display_path,
    error::EngineError,
    source_exposure::pin_current_for,
};

pub(crate) fn register(builder: &mut EngineBuilder, store: IngestSink, random: &dyn Random) {
    let mut key = [0; 32];
    if random.fill(&mut key).is_ok() {
        builder.register(Failures { store, key });
    }
}

struct Failures {
    store: IngestSink,
    key: [u8; 32],
}

impl UseCase for Failures {
    const NAME: &'static str = "scan.failures";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ScanFailuresRequest;
    type Response = ScanFailures;

    fn check_scope(&self, request: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        // The only source is a result version in this local GUI's store. A folder
        // reference must pin the request's own version, before either is looked up.
        if request
            .under_item
            .as_ref()
            .is_some_and(|item| item.result_version != request.result_version)
        {
            return Err(EngineError::reason("command.body_invalid"));
        }
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let count = request.count.unwrap_or(200);
        if !(1..=1_000).contains(&count) {
            return Err(EngineError::reason("cursor.limit_exceeded"));
        }
        cursor::check_version(&self.key, &request)?;
        let version = pin_current_for(&self.store, &request.result_version, caller.channel)
            .map_err(|error| query_error(error.query()))?;
        if request.root_indexes.as_ref().is_some_and(|indexes| {
            indexes
                .iter()
                .any(|index| *index as usize >= version.scope_roots().len())
        }) {
            return Err(EngineError::reason("command.body_invalid"));
        }
        let start = cursor::start(&self.key, &request)?;
        let under = under_folder(&version, &request)?;
        let (total, matching) = version.scan_issues_page(
            request.root_indexes.as_deref(),
            under
                .as_ref()
                .map(|(root, parts)| (*root, parts.as_slice())),
            request.kind,
            request.failures_only.unwrap_or(false),
            start,
            count as usize,
        );
        if request.cursor.is_some() && (start == 0 || start >= total) {
            return Err(EngineError::reason("cursor.invalid"));
        }
        let rows: Vec<_> = matching
            .into_iter()
            .map(|issue| project(&version, issue))
            .collect();
        let next = start + rows.len();
        let next_cursor = (next < total)
            .then(|| cursor::encode(&self.key, &request, next as u64))
            .transpose()?;
        Ok(ScanFailures {
            result_version: request.result_version,
            rows,
            total_count: total as u64,
            next_cursor,
        })
    }
}

fn query_error(error: QueryError) -> EngineError {
    EngineError::reason(match error {
        QueryError::NotFound | QueryError::PathUnavailable => "ref.unavailable",
        QueryError::StaleVersion => "result.requery_required",
        _ => "support.internal_error",
    })
}

/// The root and root-relative components of the folder a request restricts rows to.
///
/// The folder is resolved once per request, not per row: first the reference (a read
/// error when it names nothing of this version), then its kind (a body error when it
/// is not a folder). The components are the bytes the scan recorded, which are also the
/// bytes of an issue's own path, so no comparison rule applies.
type UnderFolder<'a> = (u32, Vec<&'a [u8]>);

fn under_folder<'a>(
    version: &'a ResultVersion,
    request: &ScanFailuresRequest,
) -> Result<Option<UnderFolder<'a>>, EngineError> {
    let Some(item) = &request.under_item else {
        return Ok(None);
    };
    let detail = version.detail(item).map_err(query_error)?;
    if detail.item.kind != ResultItemKind::Directory {
        return Err(EngineError::reason("command.body_invalid"));
    }
    version
        .relative_components(item)
        .map(Some)
        .map_err(query_error)
}

fn project(version: &ResultVersion, issue: &ScanIssue) -> ScanFailureRow {
    let display_path = version
        .issue_path(issue.root_index, &issue.relative)
        .ok()
        .and_then(|path| display_path::path(&path.encoding, &path.raw.bytes().ok()?));
    ScanFailureRow {
        kind: issue.kind,
        stage: issue.stage,
        path_ref: ScanFailurePathRef {
            root_index: issue.root_index,
            relative: issue
                .relative
                .iter()
                .map(|part| PathBytes::from_bytes(part))
                .collect(),
        },
        reason_key: issue.reason_key.clone(),
        observed_at: issue.observed_at.clone(),
        display_path,
    }
}
