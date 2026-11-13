//! F15 CLI commands run through the launcher binary: exit codes, output and read-only data.

#[cfg(windows)]
use std::io::Read;
use std::{fs, io, process::Command, time::SystemTime};

mod support;
#[cfg(windows)]
use support::closed_pipe_stdio;
use support::{TempDataDir, launcher_with_temp_data};

const PRODUCT_INFO_JSON: &str = include_str!(env!("DU_PRODUCT_INFO_PATH"));
const UNSUPPORTED_LINE: &str = "DiskUnfold: unsupported arguments. See --help.\n";

fn embedded_product_info() -> io::Result<serde_json::Value> {
    serde_json::from_str(PRODUCT_INFO_JSON).map_err(io::Error::other)
}

/// Runs the launcher with isolated data and returns its raw output.
fn run_launcher(args: &[&str]) -> io::Result<std::process::Output> {
    let (mut command, _data_dir) = launcher_with_temp_data()?;
    command.args(args).output()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn version_forms_print_the_same_nine_provenance_fields() -> io::Result<()> {
    let info = embedded_product_info()?;
    let reference = run_launcher(&["--lang", "en", "--version"])?;
    assert_eq!(reference.status.code(), Some(0));
    assert!(reference.stderr.is_empty());
    let stdout = text(&reference.stdout);
    let labels: Vec<&str> = stdout
        .lines()
        .map(|line| line.split(": ").next().unwrap_or_default())
        .collect();
    assert_eq!(
        labels,
        [
            "Version",
            "Build type",
            "Source commit",
            "Build time (UTC)",
            "Target operating system",
            "Target CPU",
            "Database format",
            "Snapshot format",
            "Support matrix revision",
        ],
        "{stdout}"
    );
    if info["build"]["buildKind"] == "developer" {
        assert!(stdout.contains("Build type: Developer build"), "{stdout}");
    }
    for form in ["version", "-V"] {
        let output = run_launcher(&["--lang", "en", form])?;
        assert_eq!(output.status.code(), Some(0), "{form}");
        assert_eq!(output.stdout, reference.stdout, "{form}");
        assert!(output.stderr.is_empty(), "{form}");
    }
    Ok(())
}

#[test]
fn notices_print_the_embedded_documents_exactly() -> io::Result<()> {
    let info = embedded_product_info()?;
    for language in ["en", "ko"] {
        let output = run_launcher(&["--lang", language, "notices", "LICENSE"])?;
        assert_eq!(output.status.code(), Some(0), "{language}");
        assert_eq!(
            text(&output.stdout),
            info["notices"]["license"].as_str().unwrap_or_default(),
            "{language}"
        );
        assert!(output.stderr.is_empty(), "{language}");

        let output = run_launcher(&["--lang", language, "notices", "THIRD-PARTY-NOTICES"])?;
        match info["notices"]["thirdParty"]
            .as_str()
            .filter(|document| !document.trim().is_empty())
        {
            Some(document) => {
                assert_eq!(output.status.code(), Some(0), "{language}");
                assert_eq!(text(&output.stdout), document, "{language}");
                assert!(output.stderr.is_empty(), "{language}");
            }
            None => {
                // The document is not part of this build: one stderr line, exit 1, no stdout.
                assert_eq!(output.status.code(), Some(1), "{language}");
                assert!(output.stdout.is_empty(), "{language}");
                let stderr = text(&output.stderr);
                assert_eq!(stderr.lines().count(), 1, "{stderr}");
                assert!(stderr.ends_with('\n'), "{stderr}");
            }
        }
    }
    Ok(())
}

#[test]
fn support_prints_the_published_json_envelope_and_a_matching_text_report() -> io::Result<()> {
    let info = embedded_product_info()?;
    let output = run_launcher(&["--lang", "en", "support", "--format", "json"])?;
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = text(&output.stdout);
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    let json: serde_json::Value = serde_json::from_str(&stdout).map_err(io::Error::other)?;
    assert_eq!(json["schema"], "diskunfold.cli");
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["command"], "support");
    for field in [
        "environment",
        "combinationStatus",
        "features",
        "combinations",
    ] {
        assert!(!json["result"][field].is_null(), "{field}: {stdout}");
    }
    if info["supportMatrix"].is_null() {
        // No embedded table: nothing is reported as verified.
        assert_eq!(json["result"]["combinationStatus"], "unknown");
        assert_eq!(json["result"]["reasonKey"], "support.matrix_unavailable");
    }

    let default = run_launcher(&["--lang", "en", "support"])?;
    let explicit = run_launcher(&["--lang", "en", "support", "--format", "text"])?;
    assert_eq!(default.status.code(), Some(0));
    assert_eq!(default.stdout, explicit.stdout);
    assert!(!default.stdout.is_empty());
    assert!(default.stderr.is_empty());
    Ok(())
}

#[test]
fn information_commands_reject_bad_arguments_with_exit_two_and_one_stderr_line() -> io::Result<()> {
    for args in [
        &["notices"][..],
        &["notices", "license"][..],
        &["notices", "LICENSE", "--format", "json"][..],
        &["version", "--format", "json"][..],
        &["--version", "--format", "text"][..],
        &["support", "--format", "text", "--format", "json"][..],
        &["support", "--"][..],
        &["status"][..],
        &["--unknown-option"][..],
    ] {
        let output = run_launcher(&[&["--lang", "en"], args].concat())?;
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert_eq!(text(&output.stderr), UNSUPPORTED_LINE, "{args:?}");
    }
    // A missing option value is a usage error as well.
    for args in [&["support", "--format"][..], &["--lang"][..]] {
        let output = run_launcher(
            &[&["--lang", "en"][..], args].concat()
                [..args.len() + if args == ["--lang"] { 0 } else { 2 }],
        )?;
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert_eq!(text(&output.stderr).lines().count(), 1, "{args:?}");
    }
    Ok(())
}

#[test]
fn scan_requires_roots_and_never_starts_without_an_engine() -> io::Result<()> {
    for args in [
        &["scan"][..],
        &["scan", "root"][..],
        &["scan", "--exclude"][..],
    ] {
        let output = run_launcher(&[&["--lang", "en"], args].concat())?;
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert_eq!(text(&output.stderr), UNSUPPORTED_LINE, "{args:?}");
    }
    Ok(())
}

/// Names, bytes and modification times of the entries in a directory.
fn directory_state(
    directory: &std::path::Path,
) -> io::Result<Vec<(std::ffi::OsString, Vec<u8>, SystemTime)>> {
    let mut state = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        state.push((
            entry.file_name(),
            fs::read(entry.path())?,
            entry.metadata()?.modified()?,
        ));
    }
    state.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(state)
}

/// A valid SQLite file that uses the rollback journal and carries `user_version`.
fn sqlite_header_only_database(user_version: u32) -> Vec<u8> {
    let mut page = vec![0_u8; 4096];
    page[..16].copy_from_slice(b"SQLite format 3\0");
    page[16..18].copy_from_slice(&4096_u16.to_be_bytes());
    page[18] = 1;
    page[19] = 1;
    page[21] = 64;
    page[22] = 32;
    page[23] = 32;
    page[24..28].copy_from_slice(&1_u32.to_be_bytes());
    page[28..32].copy_from_slice(&1_u32.to_be_bytes());
    page[44..48].copy_from_slice(&4_u32.to_be_bytes());
    page[56..60].copy_from_slice(&1_u32.to_be_bytes());
    page[60..64].copy_from_slice(&user_version.to_be_bytes());
    page[92..96].copy_from_slice(&1_u32.to_be_bytes());
    // An empty table leaf on page 1.
    page[100] = 0x0d;
    page[105..107].copy_from_slice(&4096_u16.to_be_bytes());
    page
}

#[test]
fn every_command_leaves_missing_older_newer_and_corrupt_databases_untouched() -> io::Result<()> {
    let commands: &[&[&str]] = &[
        &["--help"],
        &["--version"],
        &["version"],
        &["notices", "LICENSE"],
        &["notices", "THIRD-PARTY-NOTICES"],
        &["support"],
        &["support", "--format", "json"],
        &["notices"],
        &["support", "--format", "yaml"],
        &["scan", "root"],
        &["scan"],
        &["status"],
    ];
    let databases: [(&str, Option<Vec<u8>>); 4] = [
        ("older", Some(sqlite_header_only_database(0))),
        ("newer", Some(sqlite_header_only_database(99))),
        ("corrupt", Some(b"not a database".to_vec())),
        ("missing", None),
    ];
    for (label, database) in databases {
        for args in commands {
            let (mut command, data_dir) = launcher_with_temp_data()?;
            let path = data_dir.path().join("diskunfold.db");
            if let Some(bytes) = &database {
                fs::write(&path, bytes)?;
            }
            let before = directory_state(data_dir.path())?;
            let output = command.args(*args).output()?;
            assert!(output.status.code().is_some(), "{label} {args:?}");
            assert_eq!(
                directory_state(data_dir.path())?,
                before,
                "{label} {args:?}"
            );
            assert_eq!(path.exists(), database.is_some(), "{label} {args:?}");
        }
    }

    // A data directory that does not exist is not created either.
    for args in commands {
        let parent = TempDataDir::new()?;
        let missing = parent.path().join("never-created");
        let mut command = Command::new(env!("CARGO_BIN_EXE_diskunfold"));
        command.env("DISKUNFOLD_DATA_DIR", &missing).args(*args);
        let output = command.output()?;
        assert!(output.status.code().is_some(), "{args:?}");
        assert!(!missing.exists(), "{args:?}");
        assert!(directory_state(parent.path())?.is_empty(), "{args:?}");
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn every_output_command_returns_exit_five_when_stdout_is_closed() -> io::Result<()> {
    use std::process::Stdio;

    for language in ["en", "ko"] {
        for args in [
            &["version"][..],
            &["notices", "LICENSE"][..],
            &["support"][..],
            &["support", "--format", "json"][..],
        ] {
            let (mut command, _data_dir) = launcher_with_temp_data()?;
            let mut child = command
                .args(["--lang", language])
                .args(args)
                .stdin(Stdio::null())
                .stdout(closed_pipe_stdio()?)
                .stderr(Stdio::piped())
                .spawn()?;
            let mut stderr = Vec::new();
            child
                .stderr
                .take()
                .ok_or_else(|| io::Error::other("stderr pipe missing"))?
                .read_to_end(&mut stderr)?;
            assert_eq!(child.wait()?.code(), Some(5), "{language} {args:?}");
            let stderr = text(&stderr);
            assert_eq!(stderr.lines().count(), 1, "{language} {args:?}: {stderr}");
            assert!(!stderr.contains("panicked"), "{stderr}");
        }
    }
    Ok(())
}
