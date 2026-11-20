//! The scan command (F02 FR1.HD2.cli.DD1; F39 FR1.HD2, FR9, FR10, FR12; #222).
//!
//! The command assembles the engine the app uses, reviews the scope and starts
//! the scan as the screen does, writes the accepted job to stderr, and waits
//! for the end. The first interrupt cancels the job; a second one stops waiting.
//! The result summary goes to stdout once, after the end. This first version
//! records no history: it warns instead and keeps the end state.
use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
    thread,
    time::Duration,
};

use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id, OsPath, PathBytes, PathEncoding,
    analysis_ipc::{ScopeBoundary, ScopeDraft, ScopeExclusion, ScopeSource},
    job::JobState,
    ports::Random,
};
use du_engine_api::api::{CallerCtx, Channel, Engine, EngineBuilder};
use du_platform_os::{entropy::OsRandom, interrupt::Interrupts};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    args::{OutputFormat, Scan},
    diagnostics::{write_error_line, write_stderr_line},
    exit_code::{self, End, Failure},
    outbound::CliOutbound,
    product_output::Messages,
};

mod history;
use history::{DeferredHistory, Outcome as HistoryOutcome};
mod refusal;
use refusal::{fail, fail_with_localized, path_link_message};

struct HistoryRun<'a> {
    history: &'a DeferredHistory,
    data_directory: Option<PathBuf>,
}

/// How often the command reads the job's state while it waits.
const POLL: Duration = Duration::from_millis(200);
// The fields of each response the command uses; the outbound filter sends no more.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScopeReview {
    review_fingerprint: String,
    targets: Vec<ReviewTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewTarget {
    access: String,
    reason_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanStart {
    job_ref: Id,
    existing: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisJobStatus {
    job_ref: Id,
    state: JobState,
    result_version: Option<Id>,
    terminal: Option<Terminal>,
}

#[derive(Deserialize)]
struct Terminal {
    completed: u64,
    failed: u64,
    skipped: u64,
}

#[derive(Deserialize)]
struct AnalysisJobControl {
    outcome: String,
}

/// Assembles the engine the app uses, on this platform's ports.
pub(crate) fn engine() -> Result<(Engine, Arc<DeferredHistory>), String> {
    let mut builder = EngineBuilder::default();
    let history = Arc::new(DeferredHistory::default());
    du_engine_api::desktop::register(
        &mut builder,
        ports::roots(),
        ports::volumes(),
        history.clone(),
        &OsRandom,
    )
    .map_err(|error| error.to_string())?;
    builder.with_outbound_filter(CliOutbound);
    builder
        .build()
        .map(|engine| (engine, history))
        .map_err(|error| format!("{error:?}"))
}

#[cfg(windows)]
mod ports {
    use du_core_contracts::{scan_observation::ScopeRootPort, volume::VolumePort};
    use du_platform_os::{volume::WindowsVolumes, windows_observation::WindowsScopeRoots};
    use std::sync::Arc;
    pub(super) fn roots() -> Arc<dyn ScopeRootPort> {
        Arc::new(WindowsScopeRoots)
    }
    pub(super) fn volumes() -> Arc<dyn VolumePort> {
        Arc::new(WindowsVolumes)
    }
}

#[cfg(target_os = "linux")]
mod ports {
    use du_core_contracts::{scan_observation::ScopeRootPort, volume::VolumePort};
    use du_platform_os::{linux_observation::LinuxScopeRoots, volume::LinuxVolumes};
    use std::sync::Arc;
    pub(super) fn roots() -> Arc<dyn ScopeRootPort> {
        Arc::new(LinuxScopeRoots)
    }
    pub(super) fn volumes() -> Arc<dyn VolumePort> {
        Arc::new(LinuxVolumes)
    }
}

#[cfg(target_os = "macos")]
mod ports {
    use du_core_contracts::{scan_observation::ScopeRootPort, volume::VolumePort};
    use du_platform_os::{macos_observation::MacScopeRoots, volume::MacVolumes};
    use std::sync::Arc;
    pub(super) fn roots() -> Arc<dyn ScopeRootPort> {
        Arc::new(MacScopeRoots)
    }
    pub(super) fn volumes() -> Arc<dyn VolumePort> {
        Arc::new(MacVolumes)
    }
}

/// A path argument as the platform gave it; a relative one is taken from the
/// current folder, without opening anything.
fn os_path(argument: &OsString) -> Result<OsPath, &'static str> {
    let absolute = std::path::absolute(argument).map_err(|_| "path.undecodable")?;
    let display = absolute.to_string_lossy().into_owned();
    let lossy = absolute.to_str().is_none();
    #[cfg(windows)]
    let (encoding, raw) = {
        use std::os::windows::ffi::OsStrExt;
        let raw: Vec<u8> = absolute
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        (PathEncoding::WindowsUtf16le, raw)
    };
    #[cfg(unix)]
    let (encoding, raw) = {
        use std::os::unix::ffi::OsStrExt;
        (PathEncoding::Unix, absolute.as_os_str().as_bytes().to_vec())
    };
    Ok(OsPath {
        encoding,
        raw: PathBytes::from_bytes(&raw),
        display,
        lossy,
    })
}

/// An `--exclude` value: an absolute path excludes that folder, anything else
/// is a name pattern.
fn exclusion(argument: &OsString) -> Result<ScopeExclusion, &'static str> {
    if std::path::Path::new(argument).is_absolute() {
        return os_path(argument).map(|path| ScopeExclusion::Path { path });
    }
    let pattern = argument.to_str().ok_or("path.undecodable")?;
    Ok(ScopeExclusion::NamePattern {
        pattern: pattern.to_owned(),
    })
}

/// The scope draft of a parsed scan, submitted from the command line.
pub(crate) fn draft(scan: &Scan) -> Result<ScopeDraft, &'static str> {
    Ok(ScopeDraft {
        includes: scan.roots().iter().map(os_path).collect::<Result<_, _>>()?,
        exclusions: scan
            .excludes()
            .iter()
            .map(exclusion)
            .collect::<Result<_, _>>()?,
        boundary: ScopeBoundary::CurrentVolume,
        included_mounts: Vec::new(),
        source: ScopeSource::Cli,
    })
}

/// One engine call on the command-line channel.
fn call<T: DeserializeOwned>(engine: &Engine, name: &str, body: Value) -> Result<T, Failure> {
    let mut bytes = [0_u8; 16];
    OsRandom.fill(&mut bytes).map_err(|_| Failure::internal())?;
    let correlation_id = Id::from_v7_bytes(bytes);
    let caller = CallerCtx::new(Channel::Cli, correlation_id.clone(), None);
    let response = engine.call(
        CallEnvelope {
            name: name.to_owned(),
            contract_version: ENVELOPE_VERSION,
            correlation_id,
            body,
        },
        &caller,
    );
    if let Some(error) = response.error {
        return Err(Failure {
            reason: error.reason_key,
            code: error.code,
        });
    }
    serde_json::from_value(response.body).map_err(|_| Failure::internal())
}

/// Runs a scan to its end and writes its summary.
pub(crate) fn run(
    engine: &Engine,
    history: &DeferredHistory,
    scan: &Scan,
    format: OutputFormat,
    message: &Messages<'_>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> ExitCode {
    let data_directory = du_platform_os::application_data::directory();
    run_with_data_directory(
        engine,
        HistoryRun {
            history,
            data_directory,
        },
        scan,
        format,
        message,
        stdout,
        stderr,
    )
}

fn run_with_data_directory(
    engine: &Engine,
    history_run: HistoryRun<'_>,
    scan: &Scan,
    format: OutputFormat,
    message: &Messages<'_>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> ExitCode {
    let interrupts = Interrupts::install().ok();
    let draft = match draft(scan) {
        Ok(draft) => draft,
        Err(reason) => return fail(stderr, message, &Failure::argument(reason)),
    };
    let started = match start(engine, draft) {
        Ok(started) => started,
        Err(failure) => {
            let link_message = (failure.reason == "path.unresolvable")
                .then(|| path_link_message(scan, message))
                .flatten();
            return fail_with_localized(stderr, message, &failure, link_message.as_deref());
        }
    };
    write_stderr_line(stderr, &accepted(message, &started));
    let end = match wait(engine, &started.job_ref, interrupts, message, stderr) {
        Waited::Ended(status) => status,
        Waited::Abandoned => return exit_code::exit(End::Interrupted),
        Waited::Failed(failure) => return fail(stderr, message, &failure),
    };
    let interrupt_count = interrupts.map_or(0, Interrupts::count);
    let written = summary(stdout, message, format, &end).and_then(|()| stdout.flush());
    if written.is_err() {
        let _ = call::<AnalysisJobControl>(engine, "job.cancel", json!({"jobRef": end.job_ref}));
        return exit_code::exit(End::OutputClosed);
    }
    match history::record(
        history_run.history,
        &end.job_ref,
        history_run.data_directory.as_deref(),
        interrupts,
        interrupt_count,
    ) {
        HistoryOutcome::Recorded => {}
        HistoryOutcome::Refused => history_warning(
            stderr,
            message,
            "history.not_recorded",
            "cli.scan.history_not_recorded",
        ),
        HistoryOutcome::Busy => {
            history_warning(stderr, message, "db.busy", "cli.scan.history_busy")
        }
        HistoryOutcome::DiskFull => history_warning(
            stderr,
            message,
            "db.disk_full",
            "cli.scan.history_disk_full",
        ),
        HistoryOutcome::Interrupted => {
            history_warning(
                stderr,
                message,
                "history.interrupted",
                "cli.scan.history_interrupted",
            );
            return exit_code::exit(End::Interrupted);
        }
    }
    exit_code::exit(End::Finished(end.state))
}

fn history_warning(
    stderr: &mut impl Write,
    message: &Messages<'_>,
    reason: &str,
    message_key: &str,
) {
    write_stderr_line(
        stderr,
        &format!("DiskUnfold: {reason}: {}", text(message, message_key)),
    );
}

/// Reviews the scope as the screen does and starts the scan with its fingerprint.
/// A path the review cannot open stops the command before any job, as a usage
/// error with the target's own reason.
fn start(engine: &Engine, draft: ScopeDraft) -> Result<ScanStart, Failure> {
    let review: ScopeReview = call(
        engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    if let Some(target) = review
        .targets
        .into_iter()
        .find(|target| target.access != "accessible")
    {
        return Err(Failure::argument(
            target
                .reason_key
                .as_deref()
                .unwrap_or("scope.target_unavailable"),
        ));
    }
    call(
        engine,
        "scan.start",
        json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
    )
}

enum Waited {
    Ended(Box<AnalysisJobStatus>),
    /// A second interrupt stopped the wait.
    Abandoned,
    Failed(Failure),
}

/// Waits for the job's confirmed end. The first interrupt cancels the job and
/// the wait goes on to its real end; a second one stops waiting.
fn wait(
    engine: &Engine,
    job: &Id,
    interrupts: Option<Interrupts>,
    message: &Messages<'_>,
    stderr: &mut impl Write,
) -> Waited {
    let seen = interrupts.map_or(0, Interrupts::count);
    let mut cancelled = false;
    loop {
        let requests = interrupts.map_or(0, Interrupts::count).saturating_sub(seen);
        if requests >= 2 {
            return Waited::Abandoned;
        }
        if requests == 1 && !cancelled {
            cancelled = true;
            let control: Result<AnalysisJobControl, _> =
                call(engine, "job.cancel", json!({"jobRef": job}));
            let outcome = control.map_or_else(|failure| failure.reason, |control| control.outcome);
            write_stderr_line(
                stderr,
                &format!("{} ({outcome})", text(message, "cli.scan.cancel_requested")),
            );
        }
        let status: AnalysisJobStatus = match call(engine, "job.status", json!({"jobRef": job})) {
            Ok(status) => status,
            Err(failure) => return Waited::Failed(failure),
        };
        if status.terminal.is_some() {
            return Waited::Ended(Box::new(status));
        }
        thread::sleep(POLL);
    }
}

/// A localized message, or its key when the bundle lacks it.
fn text(message: &Messages<'_>, key: &str) -> String {
    message.resolve(key).unwrap_or_else(|| key.to_owned())
}

/// The accepted record: the job reference, and whether it joined a running scan.
fn accepted(message: &Messages<'_>, started: &ScanStart) -> String {
    let label = text(message, "cli.scan.accepted");
    if started.existing {
        format!(
            "DiskUnfold: {label}: {} · {}",
            started.job_ref.as_str(),
            text(message, "cli.scan.existing")
        )
    } else {
        format!("DiskUnfold: {label}: {}", started.job_ref.as_str())
    }
}

/// The wire name of a job state, as machine output writes it.
fn state_name(state: JobState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Writes the job summary once: end state, completed, failed and skipped
/// items, and the result version (F39 FR1.HD2.cli.DD1).
fn summary(
    stdout: &mut impl Write,
    message: &Messages<'_>,
    format: OutputFormat,
    end: &AnalysisJobStatus,
) -> io::Result<()> {
    let (completed, failed, skipped) = end.terminal.as_ref().map_or((0, 0, 0), |terminal| {
        (terminal.completed, terminal.failed, terminal.skipped)
    });
    let state = state_name(end.state);
    match format {
        OutputFormat::Json => {
            serde_json::to_writer(
                &mut *stdout,
                &json!({
                    "schema": "diskunfold.cli",
                    "schemaVersion": 1,
                    "command": "scan",
                    "result": {
                        "jobRef": end.job_ref,
                        "state": state,
                        "completed": completed,
                        "failed": failed,
                        "skipped": skipped,
                        "resultVersion": end.result_version,
                    },
                }),
            )?;
            writeln!(stdout)
        }
        OutputFormat::Text => {
            let version = end
                .result_version
                .as_ref()
                .map_or("-", |version| version.as_str());
            writeln!(
                stdout,
                "{}: {}",
                text(message, "cli.scan.state"),
                text(message, &format!("cli.scan.state_{state}"))
            )?;
            writeln!(
                stdout,
                "{}: {completed}",
                text(message, "cli.scan.completed")
            )?;
            writeln!(stdout, "{}: {failed}", text(message, "cli.scan.failed"))?;
            writeln!(stdout, "{}: {skipped}", text(message, "cli.scan.skipped"))?;
            writeln!(
                stdout,
                "{}: {version}",
                text(message, "cli.scan.result_version")
            )
        }
    }
}

#[cfg(test)]
mod tests;
