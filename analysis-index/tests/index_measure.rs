//! Time and peak memory of the result index for representative trees.
//!
//! This is a measurement, not a check: the test is ignored and asserts only
//! that the items it recorded were published. Run one tree per process, so
//! the process peak belongs to that tree:
//!
//! ```sh
//! cargo test --locked --release -p du-analysis-index --test index_measure --no-run
//! F03_MEASURE_TREE=mixed <target>/release/deps/index_measure-<hash> --ignored --nocapture
//! ```
//!
//! `F03_MEASURE_TREE` is `wide` (one directory with 50,000 subdirectories,
//! four files in every directory), `deep` (one chain of 4,000 directories,
//! four files in every directory) or `mixed` (`F03_MEASURE_DIRS` directories,
//! 100,001 by default, each below the previous directory or a random earlier
//! one, with 0 to 23 files). `F03_MEASURE_EVIDENCE=1` adds what a platform
//! adapter supplies for every file: allocated size, file identity, link count
//! and an observation time.
//!
//! The run records every item, publishes one partial result of the whole
//! tree and seals the build while that partial result is still held. It
//! prints one `F03_MEASURE_RESULT` line; each peak is the process peak since
//! start (Windows `PeakWorkingSet64`, Linux `VmHWM`), so it includes the
//! earlier phases. `record_s` is the whole recording phase, most of which is
//! this test building path vectors; `index_s` is the part spent inside the
//! index calls.
use du_analysis_index::{
    BuildError, BuildHandle, FileIdentity, IngestSink, Observation, ObservedAttribute,
    SharingEvidence, SpaceObservation,
};
use du_core_contracts::result::{ResultCompleteness, ResultVersionRead};
use du_core_contracts::{ByteSize, Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue, Timestamp};
use std::time::{Duration, Instant};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn err(error: BuildError) -> String {
    format!("{error:?}")
}
fn size(basis: SizeBasis, evidence: SizeEvidence, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(bytes),
        },
    }
}

/// Records items into the build and counts what a path-keyed store would hold.
struct Recorder {
    build: BuildHandle,
    evidence: Option<Timestamp>,
    reasons: bool,
    items: u64,
    components: u64,
    max_depth: usize,
    /// Time spent inside the index calls, without building their arguments.
    index: Duration,
}

impl Recorder {
    fn directory(&mut self, path: &[Vec<u8>]) -> TestResult {
        self.count(path.len());
        let path = path.to_vec();
        let started = Instant::now();
        let added = self.build.add_directory(path);
        self.index += started.elapsed();
        added.map_err(err)?;
        Ok(())
    }

    fn file(&mut self, directory: &[Vec<u8>], name: String) -> TestResult {
        let mut path = directory.to_vec();
        path.push(name.into_bytes());
        self.count(path.len());
        let observation = Observation {
            path: path.clone(),
            size: Some(size(SizeBasis::Logical, SizeEvidence::Measured, 1)),
        };
        let Some(time) = &self.evidence else {
            let started = Instant::now();
            let added = self.build.add_file(observation);
            self.index += started.elapsed();
            added.map_err(err)?;
            if self.reasons {
                self.build
                    .set_attribute_unknown(
                        0,
                        &path,
                        ObservedAttribute::ModifiedAt,
                        du_core_contracts::scan_observation::attribute_reason::QUERY_FAILED,
                    )
                    .map_err(err)?;
            }
            return Ok(());
        };
        let space = SpaceObservation {
            allocated: Some(size(SizeBasis::Allocated, SizeEvidence::OsReported, 4096)),
            identity: Some(FileIdentity {
                volume: b"volume-0001".to_vec(),
                file: self.items.to_le_bytes().to_vec(),
            }),
            link_count: Some(1),
            sharing: SharingEvidence::Unknown,
        };
        let time = time.clone();
        let started = Instant::now();
        let added = self
            .build
            .add_with_space(observation, space)
            .and_then(|()| self.build.set_item_observed_at(0, &path, time));
        self.index += started.elapsed();
        added.map_err(err)?;
        if self.reasons {
            self.build
                .set_attribute_unknown(
                    0,
                    &path,
                    ObservedAttribute::ModifiedAt,
                    du_core_contracts::scan_observation::attribute_reason::QUERY_FAILED,
                )
                .map_err(err)?;
        }
        Ok(())
    }

    fn files(&mut self, directory: &[Vec<u8>], count: u32) -> TestResult {
        (0..count).try_for_each(|n| self.file(directory, format!("file-{n:08}")))
    }

    fn count(&mut self, depth: usize) {
        self.items += 1;
        self.components += depth as u64;
        self.max_depth = self.max_depth.max(depth);
    }
}

fn wide(recorder: &mut Recorder) -> TestResult {
    recorder.directory(&[])?;
    recorder.files(&[], 4)?;
    for n in 0..50_000 {
        let path = [format!("directory-{n:08}").into_bytes()];
        recorder.directory(&path)?;
        recorder.files(&path, 4)?;
    }
    Ok(())
}

fn deep(recorder: &mut Recorder) -> TestResult {
    let mut path = Vec::new();
    recorder.directory(&path)?;
    recorder.files(&path, 4)?;
    for n in 0..4_000 {
        path.push(format!("directory-{n:08}").into_bytes());
        recorder.directory(&path)?;
        recorder.files(&path, 4)?;
    }
    Ok(())
}

/// A fixed pseudo-random sequence, so every run records the same tree.
struct Sequence(u64);

impl Sequence {
    fn below(&mut self, bound: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (((self.0 >> 33) * u64::from(bound)) >> 31) as u32
    }
}

fn mixed(recorder: &mut Recorder, directories: u32) -> TestResult {
    const MAX_DEPTH: u16 = 36;
    let mut sequence = Sequence(0x00F0_3F03);
    // Parent and depth of every directory; directory 0 is the root.
    let mut parents: Vec<(u32, u16)> = Vec::with_capacity(directories as usize);
    let mut path: Vec<Vec<u8>> = Vec::new();
    for index in 0..directories {
        let parent = match index {
            0 => 0,
            _ if sequence.below(2) == 0 && parents[index as usize - 1].1 < MAX_DEPTH => index - 1,
            _ => sequence.below(index),
        };
        let depth = if index == 0 {
            0
        } else {
            parents[parent as usize].1 + 1
        };
        parents.push((parent, depth));
        path.clear();
        let mut current = index;
        while current != 0 {
            path.push(format!("directory-{current:08}").into_bytes());
            current = parents[current as usize].0;
        }
        path.reverse();
        recorder.directory(&path)?;
        let files = sequence.below(24);
        recorder.files(&path, files)?;
    }
    Ok(())
}

/// Process peak working set since start (Windows), not a per-phase allocation.
#[cfg(windows)]
fn peak_memory() -> Option<u64> {
    let query = format!("(Get-Process -Id {}).PeakWorkingSet64", std::process::id());
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &query])
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

/// Current process resident set in bytes (macOS `ps` reports KiB).
#[cfg(target_os = "macos")]
fn resident_memory() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|kib| kib * 1024)
}

#[cfg(not(target_os = "macos"))]
fn resident_memory() -> Option<u64> {
    peak_memory()
}

/// Process peak resident set since start (Linux `VmHWM`).
#[cfg(target_os = "linux")]
fn peak_memory() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn peak_memory() -> Option<u64> {
    None
}

fn peak() -> String {
    peak_memory().map_or_else(|| "na".into(), |bytes| bytes.to_string())
}

#[test]
#[ignore = "measurement: set F03_MEASURE_TREE and run with --ignored --nocapture"]
fn index_measure() -> TestResult {
    let tree = std::env::var("F03_MEASURE_TREE")?;
    let with_evidence = std::env::var("F03_MEASURE_EVIDENCE").is_ok_and(|value| value == "1");
    let with_reasons = std::env::var("F03_MEASURE_REASONS").is_ok_and(|value| value == "1");
    let directories = match std::env::var("F03_MEASURE_DIRS") {
        Ok(value) => value.parse()?,
        Err(_) => 100_001,
    };
    let sink = IngestSink::default();
    let mut recorder = Recorder {
        build: sink.open(id(1)?).map_err(err)?,
        evidence: with_evidence
            .then(|| Timestamp::parse("2026-11-15T00:00:00.000Z"))
            .transpose()?,
        reasons: with_reasons,
        items: 0,
        components: 0,
        max_depth: 0,
        index: Duration::ZERO,
    };
    let start_peak = peak();

    let started = Instant::now();
    match tree.as_str() {
        "wide" => wide(&mut recorder)?,
        "deep" => deep(&mut recorder)?,
        "mixed" => mixed(&mut recorder, directories)?,
        other => return Err(format!("unknown F03_MEASURE_TREE {other}").into()),
    }
    let record = started.elapsed();
    let record_peak = peak();
    let record_resident = resident_memory().map_or_else(|| "na".into(), |bytes| bytes.to_string());
    if std::env::var("F03_MEASURE_RECORD_ONLY").is_ok_and(|value| value == "1") {
        println!(
            "F03_MEASURE_RECORD_ONLY tree={tree} items={} components={} max_depth={} \
             record_peak={record_peak} record_resident={record_resident}",
            recorder.items, recorder.components, recorder.max_depth,
        );
        return Ok(());
    }

    let started = Instant::now();
    let partial = recorder.build.publish_partial(id(2)?).map_err(err)?;
    let publish = started.elapsed();
    let publish_peak = peak();
    let publish_resident = resident_memory().map_or_else(|| "na".into(), |bytes| bytes.to_string());
    drop(partial);

    let started = Instant::now();
    let sealed = recorder
        .build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let seal = started.elapsed();
    let seal_peak = peak();
    let seal_resident = resident_memory().map_or_else(|| "na".into(), |bytes| bytes.to_string());

    assert_eq!(sealed.items().len() as u64, recorder.items);
    let items = recorder.items;
    let components = recorder.components;
    let max_depth = recorder.max_depth;
    let index_time = recorder.index;
    let released_resident =
        resident_memory().map_or_else(|| "na".into(), |bytes| bytes.to_string());
    println!(
        "F03_MEASURE_RESULT tree={tree} evidence={} reasons={} items={} components={} max_depth={} \
         start_peak={start_peak} record_s={:.3} index_s={:.3} record_peak={record_peak} \
         record_resident={record_resident} publish_s={:.3} publish_peak={publish_peak} \
         publish_resident={publish_resident} seal_s={:.3} seal_peak={seal_peak} \
         seal_resident={seal_resident} handle_dropped_resident={released_resident}",
        u8::from(with_evidence),
        u8::from(with_reasons),
        items,
        components,
        max_depth,
        record.as_secs_f64(),
        index_time.as_secs_f64(),
        publish.as_secs_f64(),
        seal.as_secs_f64(),
    );
    Ok(())
}
