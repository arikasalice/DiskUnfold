//! GUI-only paging over one immutable scan result.
mod cursor;

use du_analysis_index::{IngestSink, QueryError, ResultVersion};
use du_core_contracts::{
    PathBytes,
    ports::Random,
    scan_failures::{
        ScanFailurePathRef, ScanFailureRow, ScanFailures, ScanFailuresRequest, ScanIssue,
    },
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    browse::display,
    error::EngineError,
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

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        // The only source is a result version in this local GUI's store.
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let count = request.count.unwrap_or(200);
        if !(1..=1_000).contains(&count) {
            return Err(EngineError::reason("cursor.limit_exceeded"));
        }
        let start = cursor::start(&self.key, &request)?;
        let version = self
            .store
            .pin_current(&request.result_version)
            .map_err(query_error)?;
        let issues = version.scan_issues();
        let matching = issues
            .iter()
            .filter(|row| request.kind.is_none_or(|kind| row.kind == kind));
        let total = request
            .kind
            .map_or(issues.len(), |_| matching.clone().count());
        if request.cursor.is_some() && (start == 0 || start >= total) {
            return Err(EngineError::reason("cursor.invalid"));
        }
        let rows: Vec<_> = matching
            .skip(start)
            .take(count as usize)
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
        QueryError::NotFound | QueryError::StaleVersion => "ref.unavailable",
        _ => "support.internal_error",
    })
}

fn project(version: &ResultVersion, issue: &ScanIssue) -> ScanFailureRow {
    let display_path = version
        .issue_path(issue.root_index, &issue.relative)
        .ok()
        .and_then(|path| display(&path.encoding, &path.raw.bytes().ok()?));
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
