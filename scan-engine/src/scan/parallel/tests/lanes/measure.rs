//! Opt-in release measurement of the lane queue capacity (F02 NFR1,
//! NFR2.HD1.app-host.DD1, NFR3.HD1.app-host.DD1).
//!
//! `queue_capacity_grid` runs every configuration of a plan in a new process
//! of this test binary, so each process peak memory belongs to one run. The
//! child `queue_capacity_run` collects a computed tree once, either through the
//! production forest entry point with the result index (`mode=full`) or with
//! the same workers and a consumer that only counts events (`mode=collector`).
//! See release-packaging/quality/f02-queue-capacity-measurement.md.
use super::*;
use crate::scan::parallel::lanes::cost;
use std::{collections::BTreeMap, process::Command};
mod plan;
mod tree;
use tree::{Kind, Source, TreeShape};

const CONFIG: &str = "F02_MEASURE_CONFIG";
const RESULT: &str = "F02_MEASURE_RESULT";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Full,
    Collector,
    /// No collection: the test harness and process baseline.
    Idle,
}

/// One measured configuration; `dirs` is per root and capacity 0 is unbounded.
#[derive(Clone, Copy, Debug)]
struct Config {
    kind: Kind,
    dirs: u64,
    files: u32,
    batch: usize,
    workers: usize,
    limit: usize,
    capacity: usize,
    lanes: usize,
    delay_us: u64,
    mode: Mode,
}

impl Config {
    fn render(&self) -> String {
        let mode = match self.mode {
            Mode::Full => "full",
            Mode::Collector => "collector",
            Mode::Idle => "idle",
        };
        format!(
            "kind={} dirs={} files={} batch={} workers={} limit={} capacity={} lanes={} delay_us={} mode={mode}",
            self.kind.name(),
            self.dirs,
            self.files,
            self.batch,
            self.workers,
            self.limit,
            self.capacity,
            self.lanes,
            self.delay_us,
        )
    }

    fn parse(text: &str) -> Result<Self, TestError> {
        let fields: BTreeMap<&str, &str> = text
            .split_whitespace()
            .filter_map(|field| field.split_once('='))
            .collect();
        let get = |key: &str| fields.get(key).copied().ok_or(format!("missing {key}"));
        let number = |key: &str| -> Result<u64, TestError> { Ok(get(key)?.parse()?) };
        let mode = match get("mode")? {
            "full" => Mode::Full,
            "collector" => Mode::Collector,
            "idle" => Mode::Idle,
            other => return Err(format!("unknown mode {other}").into()),
        };
        Ok(Self {
            kind: Kind::parse(get("kind")?).ok_or("unknown kind")?,
            dirs: number("dirs")?,
            files: u32::try_from(number("files")?)?,
            batch: usize::try_from(number("batch")?)?,
            workers: usize::try_from(number("workers")?)?,
            limit: usize::try_from(number("limit")?)?,
            capacity: usize::try_from(number("capacity")?)?,
            lanes: usize::try_from(number("lanes")?)?,
            delay_us: number("delay_us")?,
            mode,
        })
    }

    fn capacity(&self) -> usize {
        if self.capacity == 0 {
            usize::MAX
        } else {
            self.capacity
        }
    }
}

/// Measurements of one run, printed as `key=value` fields.
struct Report {
    fields: Vec<(&'static str, String)>,
}

/// Job context of a full run: production-like publication cadence and progress.
struct Clocked {
    started: Instant,
    source: Arc<Source>,
    flows: Context,
}

impl JobContext for Clocked {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Continue
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        self.flows.try_start_flow()
    }
    fn monotonic_now(&self) -> Option<Duration> {
        Some(self.started.elapsed())
    }
    fn report_progress(&self, progress: du_core_contracts::job::JobProgress) {
        self.source
            .consumed
            .store(progress.completed, Ordering::Relaxed);
    }
}

fn flows() -> Context {
    Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    }
}

/// Collects through the production forest entry point into a result index.
/// Returns (recorded items without the roots, lane queue peak).
fn full(config: &Config, source: &Arc<Source>) -> Result<(u64, Option<usize>), TestError> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots = (0..config.lanes)
        .map(|index| observed_root(&format!("L:\\{index}"), index as u8 + 1))
        .collect();
    let build = sink.open_forest(ids.next_id(), roots);
    let diagnostics = crate::scan_job::ScanDiagnostics::default();
    let context = Clocked {
        started: Instant::now(),
        source: source.clone(),
        flows: flows(),
    };
    let devices = DevicePlan {
        groups: (0..config.lanes).collect(),
        limit: config.limit,
    };
    scheduler::TEST_LANE_CAPACITY.store(config.capacity(), Ordering::Release);
    let outcome = collect_open_forest_diagnostic(
        build,
        source.roots(config.lanes),
        &ids,
        &context,
        config.workers,
        None,
        (devices, Some(&diagnostics), None),
    );
    scheduler::TEST_LANE_CAPACITY.store(0, Ordering::Release);
    let version = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing result")?;
    if outcome.failure_reason_key.is_some()
        || version.completeness() != ResultCompleteness::Complete
    {
        return Err(format!(
            "incomplete: {outcome:?} gaps={:?} issues={:?}",
            version.scope_gaps(),
            version.scan_issues()
        )
        .into());
    }
    let items = version.items().len().saturating_sub(config.lanes) as u64;
    Ok((items, diagnostics.shared_ready_peak()))
}

/// Runs the production workers over the lanes with a counting consumer.
fn collector(config: &Config, source: &Arc<Source>) -> Result<(u64, Option<usize>), TestError> {
    let groups = (0..config.lanes).collect();
    let roots = source.roots(config.lanes);
    let shared = shared_over(roots, groups, config.limit, config.capacity())?;
    let items = run_workers(&shared, config.workers, BUFFERED_OBSERVATIONS, |receiver| {
        let mut items = 0;
        for event in receiver {
            if matches!(event, Event::Item(..)) {
                items += 1;
                source.consumed.store(items, Ordering::Relaxed);
            }
        }
        items
    })?;
    if shared.abort.load(Ordering::SeqCst) {
        return Err("collection aborted".into());
    }
    let peak = shared.queue.lock().map_err(|_| "queue lock")?.lanes.peak();
    Ok((items, peak))
}

fn run(config: &Config) -> Result<Report, TestError> {
    let shape = TreeShape {
        kind: config.kind,
        dirs: config.dirs,
        files: config.files,
    };
    let delay = Duration::from_micros(config.delay_us);
    let source = Source::new(shape, config.batch, delay);
    let (dirs, files, deepest) = shape.expected();
    let roots = config.lanes as u64;
    let expected = roots * (dirs + files - 1);
    cost::take_stack_peaks();
    let started = Instant::now();
    let (items, ready_peak) = match config.mode {
        Mode::Full => full(config, &source)?,
        Mode::Collector => collector(config, &source)?,
        Mode::Idle => (0, None),
    };
    let secs = started.elapsed().as_secs_f64();
    if config.mode != Mode::Idle && items != expected {
        return Err(format!("collected {items} of {expected} items").into());
    }
    let (stack_peak, stacked_peak) = cost::take_stack_peaks();
    let reads = source.reads.load(Ordering::Relaxed);
    let slept = source.slept_nanos.load(Ordering::Relaxed);
    let delay_eff_us = if reads == 0 {
        0.0
    } else {
        slept as f64 / reads as f64 / 1000.0
    };
    let fields = vec![
        ("dirs_total", (roots * dirs).to_string()),
        ("items", items.to_string()),
        ("deepest", deepest.to_string()),
        ("secs", format!("{secs:.3}")),
        (
            "items_per_sec",
            format!("{:.0}", items as f64 / secs.max(1e-9)),
        ),
        (
            "ready_peak",
            ready_peak.map_or("na".into(), |peak| peak.to_string()),
        ),
        ("stack_peak", stack_peak.to_string()),
        ("stacked_peak", stacked_peak.to_string()),
        (
            "handles_peak",
            source.live_peak.load(Ordering::Relaxed).to_string(),
        ),
        (
            "unconsumed_peak",
            source.unconsumed_peak.load(Ordering::Relaxed).to_string(),
        ),
        ("reads", reads.to_string()),
        ("delay_eff_us", format!("{delay_eff_us:.1}")),
        (
            "peak_bytes",
            peak_memory().map_or("na".into(), |bytes| bytes.to_string()),
        ),
    ];
    Ok(Report { fields })
}

/// Process peak working set since start (Windows), not a per-scan allocation.
#[cfg(windows)]
fn peak_memory() -> Option<u64> {
    let query = format!("(Get-Process -Id {}).PeakWorkingSet64", std::process::id());
    powershell(&query)?.trim().parse().ok()
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

#[cfg(windows)]
fn powershell(query: &str) -> Option<String> {
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", query])
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8(output.stdout).ok()
}

/// Host CPU load: Windows processor load percent, Linux one-minute load average.
fn host_load() -> String {
    observed_load().unwrap_or_else(|| "load=na".into())
}

#[cfg(windows)]
fn observed_load() -> Option<String> {
    powershell(
        "(Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average",
    )
    .map(|text| format!("cpu_percent={}", text.trim()))
}

#[cfg(target_os = "linux")]
fn observed_load() -> Option<String> {
    let text = std::fs::read_to_string("/proc/loadavg").ok()?;
    let load = text.split_whitespace().next()?;
    Some(format!("loadavg1={load}"))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn observed_load() -> Option<String> {
    None
}

#[test]
#[ignore = "opt-in measurement child; queue_capacity_grid starts it with a configuration"]
fn queue_capacity_run() -> Result<(), TestError> {
    let Ok(text) = std::env::var(CONFIG) else {
        eprintln!("{CONFIG} is not set; run queue_capacity_grid instead");
        return Ok(());
    };
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let report = run(&Config::parse(&text)?)?;
    let fields: Vec<String> = report
        .fields
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    println!("{RESULT} {}", fields.join(" "));
    Ok(())
}

/// Runs one configuration in a new process and returns its result fields.
fn spawn(config: &Config) -> Result<BTreeMap<String, String>, TestError> {
    let name = concat!(module_path!(), "::queue_capacity_run");
    let name = name.split_once("::").map_or(name, |(_, rest)| rest);
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            name,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CONFIG, config.render())
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find_map(|line| line.split_once(RESULT).map(|(_, fields)| fields))
        .filter(|_| output.status.success());
    let Some(line) = line else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{}: {stdout} {stderr}", config.render()).into());
    };
    Ok(line
        .split_whitespace()
        .filter_map(|field| field.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect())
}

#[test]
#[ignore = "opt-in release measurement; spawns one process per run (F02_MEASURE_PLAN)"]
fn queue_capacity_grid() -> Result<(), TestError> {
    let plan = std::env::var("F02_MEASURE_PLAN").unwrap_or_else(|_| "smoke".into());
    let reps: usize = std::env::var("F02_MEASURE_REPS").map_or(Ok(3), |text| text.parse())?;
    let filter = std::env::var("F02_MEASURE_FILTER").unwrap_or_default();
    let configs: Vec<Config> = plan::configs(&plan)?
        .into_iter()
        .filter(|config| {
            let text = config.render();
            filter.split_whitespace().all(|token| text.contains(token))
        })
        .collect();
    println!(
        "F02_MEASURE_HOST os={} arch={} logical_cpus={} plan={plan} reps={reps} runs={} {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        thread::available_parallelism().map_or(0, usize::from),
        configs.len() * reps,
        host_load(),
    );
    println!(
        "F02_MEASURE_SIZES frame={} queued_item={} observed_entry={} timed_entry={}",
        size_of::<Frame>(),
        size_of::<(u64, Frame)>(),
        size_of::<ObservedEntry>(),
        size_of::<TimedObservedEntry>(),
    );
    let mut failures = 0;
    for config in &configs {
        let mut runs = Vec::new();
        for rep in 0..reps {
            match spawn(config) {
                Ok(fields) => {
                    println!(
                        "F02_MEASURE_RUN rep={rep} {} {}",
                        config.render(),
                        plan::line(&fields)
                    );
                    runs.push(fields);
                }
                Err(error) => {
                    failures += 1;
                    println!("F02_MEASURE_FAIL rep={rep} {error}");
                }
            }
        }
        if !runs.is_empty() {
            println!(
                "F02_MEASURE_MEDIAN {} {}",
                config.render(),
                plan::median(&runs)
            );
        }
    }
    println!("F02_MEASURE_END failures={failures} {}", host_load());
    if failures == 0 {
        Ok(())
    } else {
        Err(format!("{failures} runs failed").into())
    }
}
