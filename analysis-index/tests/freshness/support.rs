//! Shared fixtures for the freshness ledger tests.
use du_analysis_index::{BuildError, BuildHandle, IngestSink};
use du_core_contracts::{
    Id, PathBytes, PathEncoding, Timestamp,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    result_events::{ResultVersionNotice, ResultVersionPublisher},
    result_freshness::{FreshnessChanged, ResultFreshnessNotice, ResultFreshnessPublisher},
};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct Failure(String);
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Failure {}
impl From<String> for Failure {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl From<&str> for Failure {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}
impl From<BuildError> for Failure {
    fn from(value: BuildError) -> Self {
        Self(format!("{value:?}"))
    }
}
pub type Check = Result<(), Failure>;

#[derive(Debug)]
pub enum Entry {
    Version(ResultVersionNotice),
    Fresh(ResultFreshnessNotice),
}

/// Records both channels in one log, and reads the store from inside each callback.
#[derive(Default)]
pub struct Log {
    pub entries: Mutex<Vec<Entry>>,
    pub sink: Mutex<Option<IngestSink>>,
    /// Whether the store answered for the current version while a freshness notice was sent.
    pub readable: Mutex<Vec<bool>>,
}

impl Log {
    pub fn fresh(&self) -> Vec<ResultFreshnessNotice> {
        let entries = self.entries.lock().map(|e| {
            e.iter()
                .filter_map(|entry| match entry {
                    Entry::Fresh(notice) => Some(notice.clone()),
                    Entry::Version(_) => None,
                })
                .collect()
        });
        entries.unwrap_or_default()
    }
    pub fn kinds(&self) -> String {
        let entries = self.entries.lock().map(|e| {
            e.iter()
                .map(|entry| match entry {
                    Entry::Version(_) => 'V',
                    Entry::Fresh(_) => 'F',
                })
                .collect()
        });
        entries.unwrap_or_default()
    }
}
impl ResultVersionPublisher for Log {
    fn publish_result_version(&self, notice: ResultVersionNotice) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(Entry::Version(notice));
        }
    }
}
impl ResultFreshnessPublisher for Log {
    fn publish_result_freshness(&self, notice: ResultFreshnessNotice) {
        let readable = match (&notice.result_version, self.sink.lock()) {
            (Some(version), Ok(sink)) => sink
                .as_ref()
                .is_some_and(|sink| sink.pin_current(version).is_ok()),
            // Without a current version the notice must not claim one.
            (None, _) => true,
            _ => false,
        };
        if let (Ok(mut seen), Ok(mut entries)) = (self.readable.lock(), self.entries.lock()) {
            seen.push(readable);
            entries.push(Entry::Fresh(notice));
        }
    }
}

pub fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
pub fn sink() -> Result<(IngestSink, Arc<Log>), String> {
    let log = Arc::new(Log::default());
    let sink = IngestSink::with_publishers(log.clone(), log.clone());
    *log.sink.lock().map_err(|_| "poisoned")? = Some(sink.clone());
    Ok((sink, log))
}
pub fn roots(count: u8) -> Vec<ResultScopeRoot> {
    (0..count)
        .map(|n| ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(&[67 + n, 0]),
            },
            volume_id: vec![n + 1],
            file_id: vec![n + 1],
        })
        .collect()
}
pub fn lineage(sink: &IngestSink, key: &str, n: u32) -> Result<Id, String> {
    sink.bind_current_analysis(key, id(n)?)
        .map_err(|e| format!("{e:?}"))
}
/// Opens a build whose every root has its root directory; `deep` roots also have a child.
pub fn open(
    sink: &IngestSink,
    lineage: &Id,
    scope: u32,
    count: u8,
    deep: &[u32],
) -> Result<BuildHandle, String> {
    let mut build = sink
        .open_forest_in_lineage(id(scope)?, roots(count), lineage.clone())
        .map_err(|e| format!("{e:?}"))?;
    for root in 0..u32::from(count) {
        build
            .add_directory_at(root, Vec::new())
            .map_err(|e| format!("{e:?}"))?;
        if deep.contains(&root) {
            build
                .add_directory_at(root, vec![b"d".to_vec()])
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    Ok(build)
}
pub fn seal(build: BuildHandle, version: u32, how: ResultCompleteness) -> Result<(), String> {
    build
        .seal_at(id(version)?, how, Timestamp::from_unix_millis(5_000))
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}
/// The state names and reasons of a notice's ranges, or none for a requery.
pub fn ranges(notice: &ResultFreshnessNotice) -> Option<Vec<(u32, String, Option<String>)>> {
    match &notice.body.changed {
        FreshnessChanged::Ranges { ranges } => Some(
            ranges
                .iter()
                .map(|r| {
                    (
                        r.root_index,
                        r.state.as_str().to_owned(),
                        r.reason_key.clone(),
                    )
                })
                .collect(),
        ),
        _ => None,
    }
}
pub fn row(index: u32, state: &str, reason: Option<&str>) -> (u32, String, Option<String>) {
    (index, state.to_owned(), reason.map(str::to_owned))
}
