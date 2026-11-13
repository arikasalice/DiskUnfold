//! Fixture-only SQLite adapter composition and reproducible timing harness.
use std::{
    fs,
    num::NonZeroUsize,
    path::Path,
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};

use du_app_db::{
    AppDb,
    recent_scope::{
        RecentError as DbError, RecentItem, RecentScopeStore, RecentSummary, RecentWrite,
    },
};
use du_core_contracts::{OsPath, PathBytes, PathEncoding};
use du_user_env::scope::recent::{
    RecentBody, RecentError, RecentRepository, RecentService, RepositoryError,
};
use serde_json::{Value, json};

const WARMUP: usize = 20;
const SAMPLES: usize = 200;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
type FixtureService = RecentService<Store, fn(&str) -> bool>;

struct Candidate {
    id: [u8; 16],
    fingerprint: [u8; 32],
    body: String,
    count: u32,
    time: i64,
}
impl RecentBody for Candidate {
    fn body(&self) -> &str {
        &self.body
    }
}
struct Store(AppDb);
fn map(e: DbError) -> RepositoryError {
    match e {
        DbError::Busy => RepositoryError::Busy,
        DbError::DiskFull => RepositoryError::Full,
        DbError::Corrupt => RepositoryError::Corrupt,
        DbError::Invalid | DbError::Storage => RepositoryError::Storage,
    }
}
impl RecentRepository for Store {
    type Summary = RecentSummary;
    type Item = RecentItem;
    type Write = Candidate;
    fn list(&mut self, cap: NonZeroUsize) -> Result<Vec<Self::Summary>, RepositoryError> {
        RecentScopeStore::new(&mut self.0, cap, &valid)
            .map_err(map)?
            .list()
            .map_err(map)
    }
    fn get(
        &mut self,
        id: &[u8; 16],
        cap: NonZeroUsize,
        validator: &dyn Fn(&str) -> bool,
    ) -> Result<Option<Self::Item>, RepositoryError> {
        RecentScopeStore::new(&mut self.0, cap, validator)
            .map_err(map)?
            .get(id)
            .map_err(map)
    }
    fn put(
        &mut self,
        item: &Candidate,
        cap: NonZeroUsize,
        validator: &dyn Fn(&str) -> bool,
    ) -> Result<(), RepositoryError> {
        let write = RecentWrite {
            id: item.id,
            fingerprint: item.fingerprint,
            body: &item.body,
            include_count: item.count,
            now_ms: item.time,
        };
        RecentScopeStore::new(&mut self.0, cap, validator)
            .map_err(map)?
            .put(&write)
            .map_err(map)
    }
}
fn valid(body: &str) -> bool {
    serde_json::from_str::<Value>(body).ok().is_some_and(|v| {
        v.get("includes")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    })
}
fn native(value: &str) -> OsPath {
    let raw = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    OsPath {
        encoding: PathEncoding::WindowsUtf16le,
        raw: PathBytes::from_bytes(&raw),
        display: value.into(),
        lossy: false,
    }
}
fn fixture(kind: &str) -> TestResult<(String, u32)> {
    let paths: Vec<String> = match kind {
        "single" => vec![r"C:\Users\Example\Documents".into()],
        "korean_long" => vec![format!(
            r"C:\Users\Example\{}\{}\자료",
            "문서".repeat(40),
            "분석".repeat(40)
        )],
        "multi" => (0..12)
            .map(|i| format!(r"C:\Data\Project-{i:02}\자료"))
            .collect(),
        _ => return Err("unknown fixture".into()),
    };
    let includes = paths.iter().map(|path| json!({"volumeId":"synthetic-volume", "entryPath":native(r"C:\"), "volumeRelativePath":native(path.trim_start_matches("C:"))})).collect::<Vec<_>>();
    let excludes = if kind == "multi" {
        (0..40).map(|i| json!({"kind":"path", "path":native(&format!(r"C:\Data\Project-{:02}\cache-{i:02}", i % 12))})).collect::<Vec<_>>()
    } else {
        vec![]
    };
    let count = u32::try_from(includes.len())?;
    Ok((
        serde_json::to_string(
            &json!({"includes":includes,"exclusions":excludes,"boundary":"current_volume","includedMounts":[]}),
        )?,
        count,
    ))
}
fn candidate(number: u64, body: &str, count: u32, time: i64) -> Candidate {
    let mut id = [0; 16];
    id[6] = 0x70;
    id[8] = 0x80;
    id[8..].copy_from_slice(&number.to_be_bytes());
    id[8] = (id[8] & 0x3f) | 0x80;
    let mut fingerprint = [0; 32];
    fingerprint[..8].copy_from_slice(&number.to_be_bytes());
    Candidate {
        id,
        fingerprint,
        body: body.into(),
        count,
        time,
    }
}
fn service(path: &Path, cap: NonZeroUsize, create: bool) -> TestResult<FixtureService> {
    let db = if create {
        AppDb::create_gui_with_history(path)
    } else {
        AppDb::open_gui_with_history(path)
    }
    .map_err(|e| format!("{e:?}"))?;
    Ok(RecentService::new(
        Store(db),
        cap,
        valid as fn(&str) -> bool,
    ))
}
fn p95(samples: &mut [Duration]) -> f64 {
    samples.sort_unstable();
    samples[(samples.len() * 95).div_ceil(100) - 1].as_secs_f64() * 1000.0
}
fn timed<T>(f: impl FnOnce() -> Result<T, RecentError>) -> (Result<T, RecentError>, Duration) {
    let now = Instant::now();
    let result = f();
    (result, now.elapsed())
}
fn record<T>(
    result: Result<T, RecentError>,
    elapsed: Duration,
    index: usize,
    samples: &mut Vec<Duration>,
    busy: &mut usize,
) -> TestResult<Option<T>> {
    match result {
        Ok(value) => {
            if index >= WARMUP {
                samples.push(elapsed);
            }
            Ok(Some(value))
        }
        Err(RecentError::Repository(RepositoryError::Busy)) => {
            *busy += 1;
            Ok(None)
        }
        Err(error) => Err(format!("unexpected service failure: {error:?}").into()),
    }
}
fn bytes(path: &Path) -> std::io::Result<u64> {
    fs::metadata(path).map(|metadata| metadata.len())
}

fn verify_adapter_semantics() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let cap = NonZeroUsize::new(2).ok_or("zero cap")?;
    let (first_body, first_count) = fixture("single")?;
    let (different_body, different_count) = fixture("multi")?;
    let mut service = service(&dir.path().join("verify.db"), cap, true)?;
    let first = candidate(1, &first_body, first_count, 1);
    let second = candidate(2, &first_body, first_count, 2);
    service.put(None, &first).map_err(|e| format!("{e:?}"))?;
    service.put(None, &second).map_err(|e| format!("{e:?}"))?;
    let mut update = candidate(99, &different_body, different_count, 3);
    update.fingerprint = first.fingerprint;
    service.put(None, &update).map_err(|e| format!("{e:?}"))?;
    let stored = service
        .get(&first.id)
        .map_err(|e| format!("{e:?}"))?
        .ok_or("updated original missing")?;
    if stored.body != first_body
        || stored.summary.id != first.id
        || stored.summary.last_used_at_ms != 3
        || service
            .get(&update.id)
            .map_err(|e| format!("{e:?}"))?
            .is_some()
    {
        return Err("duplicate update changed original body or identity".into());
    }
    let third = candidate(3, &first_body, first_count, 4);
    service.put(None, &third).map_err(|e| format!("{e:?}"))?;
    if service
        .get(&second.id)
        .map_err(|e| format!("{e:?}"))?
        .is_some()
        || service
            .get(&first.id)
            .map_err(|e| format!("{e:?}"))?
            .is_none()
        || service
            .get(&third.id)
            .map_err(|e| format!("{e:?}"))?
            .is_none()
    {
        return Err("oldest eviction failed".into());
    }
    println!("semantic precheck: duplicate body/id preserved; oldest row evicted");
    Ok(())
}
fn measure(kind: &str, cap_value: usize) -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("measure.db");
    let wal = directory.path().join("measure.db-wal");
    let cap = NonZeroUsize::new(cap_value).ok_or("zero cap")?;
    let (body, count) = fixture(kind)?;
    let mut writer = service(&path, cap, true)?;
    for i in 0..cap_value {
        writer
            .put(None, &candidate(i as u64 + 1, &body, count, i as i64 + 1))
            .map_err(|e| format!("{e:?}"))?;
    }
    let mut list = Vec::new();
    let mut update = Vec::new();
    let mut eviction = Vec::new();
    let mut busy = 0;
    for index in 0..WARMUP + SAMPLES {
        let (result, duration) = timed(|| writer.list());
        if let Some(rows) = record(result, duration, index, &mut list, &mut busy)?
            && rows.len() != cap_value
        {
            return Err("incorrect list size".into());
        }
        let (result, duration) = timed(|| {
            writer.put(
                None,
                &candidate(cap_value as u64, &body, count, 10_000 + index as i64),
            )
        });
        record(result, duration, index, &mut update, &mut busy)?;
    }
    for index in 0..WARMUP + SAMPLES {
        let (result, duration) = timed(|| {
            writer.put(
                None,
                &candidate(100_000 + index as u64, &body, count, 20_000 + index as i64),
            )
        });
        record(result, duration, index, &mut eviction, &mut busy)?;
    }
    let reader = service(&path, cap, false)?;
    let barrier = Arc::new(Barrier::new(2));
    let reader_barrier = barrier.clone();
    let join = thread::spawn(move || -> Result<(Vec<Duration>, usize), String> {
        let mut reader = reader;
        let mut durations = Vec::new();
        let mut busy = 0;
        let mut failure = None;
        for index in 0..WARMUP + SAMPLES {
            reader_barrier.wait();
            let (result, elapsed) = timed(|| reader.list());
            match record(result, elapsed, index, &mut durations, &mut busy) {
                Ok(Some(rows)) if rows.len() != cap_value => {
                    failure.get_or_insert_with(|| "incorrect concurrent list size".to_owned());
                }
                Err(error) => {
                    failure.get_or_insert_with(|| error.to_string());
                }
                _ => {}
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        Ok((durations, busy))
    });
    let mut concurrent_write = Vec::new();
    let mut concurrent_busy = 0;
    let mut failure = None;
    for index in 0..WARMUP + SAMPLES {
        barrier.wait();
        let (result, duration) = timed(|| {
            writer.put(
                None,
                &candidate(200_000 + index as u64, &body, count, 30_000 + index as i64),
            )
        });
        if let Err(error) = record(
            result,
            duration,
            index,
            &mut concurrent_write,
            &mut concurrent_busy,
        ) {
            failure.get_or_insert_with(|| error.to_string());
        }
    }
    let (mut concurrent_list, reader_busy) = join.join().map_err(|_| "reader panic")??;
    if let Some(error) = failure {
        return Err(error.into());
    }
    let inspect = rusqlite::Connection::open(&path)?;
    let pages: i64 = inspect.pragma_query_value(None, "page_count", |row| row.get(0))?;
    let page_size: i64 = inspect.pragma_query_value(None, "page_size", |row| row.get(0))?;
    let sqlite: String = inspect.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;
    let min_count = [
        list.len(),
        update.len(),
        eviction.len(),
        concurrent_list.len(),
        concurrent_write.len(),
    ]
    .into_iter()
    .min()
    .ok_or("no samples")?;
    if min_count == 0 {
        return Err("no successful samples".into());
    }
    println!(
        "fixture={kind} cap={cap_value} body_bytes={} samples={}/{}/{}/{}/{} busy={busy}/{concurrent_busy}/{reader_busy} list_ms={:.3} update_ms={:.3} evict_ms={:.3} concurrent_list_ms={:.3} concurrent_evict_ms={:.3} page_bytes={} db_file_bytes={} wal_bytes={} sqlite={sqlite}",
        body.len(),
        list.len(),
        update.len(),
        eviction.len(),
        concurrent_list.len(),
        concurrent_write.len(),
        p95(&mut list),
        p95(&mut update),
        p95(&mut eviction),
        p95(&mut concurrent_list),
        p95(&mut concurrent_write),
        pages * page_size,
        bytes(&path)?,
        bytes(&wal)?
    );
    Ok(())
}

pub fn run() -> TestResult<()> {
    println!(
        "warmup={WARMUP} measured={SAMPLES} seed=sequential; SQLite WAL/NORMAL, busy_timeout=5000ms; fixture validator is not a published format"
    );
    verify_adapter_semantics()?;
    for kind in ["single", "korean_long", "multi"] {
        for cap in [50, 100, 500] {
            measure(kind, cap)?;
        }
    }
    Ok(())
}
