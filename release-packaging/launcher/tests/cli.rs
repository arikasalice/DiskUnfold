use std::{
    fs,
    io::{self, Read},
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_DATA_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDataDir(PathBuf);

impl TempDataDir {
    fn new() -> io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let sequence = NEXT_DATA_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "diskunfold-cli-test-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDataDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn launcher_with_temp_data() -> io::Result<(Command, TempDataDir)> {
    let data_dir = TempDataDir::new()?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_diskunfold"));
    command.env("DISKUNFOLD_DATA_DIR", data_dir.path());
    let isolated = command.get_envs().any(|(name, value)| {
        name == "DISKUNFOLD_DATA_DIR" && value == Some(data_dir.path().as_os_str())
    });
    if !isolated || !data_dir.path().is_dir() {
        return Err(io::Error::other("isolated CLI data directory not ready"));
    }
    Ok((command, data_dir))
}

fn is_localized_settings_unavailable_line(line: &str) -> bool {
    matches!(
        line,
        "Saved settings are unavailable. Using a fallback language."
            | "저장된 설정을 사용할 수 없어 대체 언어를 사용합니다."
    )
}

fn has_any_settings_unavailable_warning(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        is_localized_settings_unavailable_line(line)
            || line == "settings_unavailable"
            || line == "cli.settings.unavailable"
    })
}

fn assert_localized_settings_unavailable_warning_once(stderr: &str) {
    assert!(
        !stderr
            .lines()
            .any(|line| { line == "settings_unavailable" || line == "cli.settings.unavailable" })
    );
    assert_eq!(
        stderr
            .lines()
            .filter(|line| is_localized_settings_unavailable_line(line))
            .count(),
        1,
        "{stderr}"
    );
}

#[cfg(windows)]
fn closed_pipe_stdio() -> io::Result<std::process::Stdio> {
    let (reader, writer) = io::pipe()?;
    drop(reader);
    Ok(std::process::Stdio::from(writer))
}

#[test]
fn launcher_prints_help_and_version_to_stdout() -> io::Result<()> {
    for (args, expected) in [
        (
            ["--lang", "en", "--help"],
            "DiskUnfold\n\nUsage: diskunfold [--lang LANGUAGE] [--help | --version]\n",
        ),
        (
            ["--lang", "ko", "--help"],
            "DiskUnfold\n\n사용법: diskunfold [--lang 언어] [--help | --version]\n",
        ),
        (
            ["--lang", "en", "--version"],
            concat!("DiskUnfold ", env!("CARGO_PKG_VERSION"), "\n"),
        ),
    ] {
        let (mut command, _data_dir) = launcher_with_temp_data()?;
        let output = command.args(args).output()?;
        assert!(output.status.success());
        assert_eq!(output.stdout.as_slice(), expected.as_bytes());
        assert!(output.stderr.is_empty());
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn help_and_version_return_exit_five_when_stdout_is_closed() -> io::Result<()> {
    use std::process::Stdio;

    for (language, flag, expected_diagnostic) in [
        ("en", "--help", "Output was interrupted.\n"),
        ("en", "--version", "Output was interrupted.\n"),
        ("ko", "--help", "출력이 중단됐습니다.\n"),
        ("ko", "--version", "출력이 중단됐습니다.\n"),
    ] {
        let (mut command, _data_dir) = launcher_with_temp_data()?;
        let closed_stdout = closed_pipe_stdio()?;
        let mut child = command
            .args(["--lang", language, flag])
            .stdin(Stdio::null())
            .stdout(closed_stdout)
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stderr = Vec::new();
        child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("stderr pipe missing"))?
            .read_to_end(&mut stderr)?;
        let status = child.wait()?;
        assert_eq!(status.code(), Some(5), "{flag}");
        assert_eq!(stderr.as_slice(), expected_diagnostic.as_bytes());
        assert!(
            !String::from_utf8_lossy(&stderr).contains("panicked"),
            "{flag}: {}",
            String::from_utf8_lossy(&stderr)
        );
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn closed_stderr_does_not_replace_output_interrupted_exit() -> io::Result<()> {
    use std::process::Stdio;

    for language in ["en", "ko"] {
        let (mut command, _data_dir) = launcher_with_temp_data()?;
        let closed_stdout = closed_pipe_stdio()?;
        let closed_stderr = closed_pipe_stdio()?;
        let mut child = command
            .args(["--lang", language, "--help"])
            .stdin(Stdio::null())
            .stdout(closed_stdout)
            .stderr(closed_stderr)
            .spawn()?;
        assert_eq!(child.wait()?.code(), Some(5), "{language}");
    }
    Ok(())
}

#[test]
fn launcher_rejects_unsupported_commands_and_trailing_arguments() -> io::Result<()> {
    for args in [&["scan"][..], &["--version", "extra"][..]] {
        let (mut command, _data_dir) = launcher_with_temp_data()?;
        let output = command.args(["--lang", "en"]).args(args).output()?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported arguments"));
    }
    Ok(())
}

#[test]
fn launcher_applies_the_language_option_on_either_side_of_help() -> io::Result<()> {
    for args in [["--lang", "KO", "--help"], ["--help", "--lang", "ko-KR"]] {
        let (mut command, _data_dir) = launcher_with_temp_data()?;
        let output = command.args(args).output()?;
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("사용법: diskunfold"));
        assert!(output.stderr.is_empty());
    }
    let (mut command, _data_dir) = launcher_with_temp_data()?;
    let output = command.args(["--lang", "fr", "--help"]).output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("locale.unsupported"));
    Ok(())
}

#[cfg(windows)]
#[test]
fn help_and_version_skip_a_corrupt_settings_database() -> io::Result<()> {
    for (arguments, expected) in [
        (vec!["--lang", "en", "--help"], "Usage: diskunfold"),
        (vec!["--lang", "ko", "--version"], "DiskUnfold "),
    ] {
        let (mut command, data_dir) = launcher_with_temp_data()?;
        fs::write(
            data_dir.path().join("diskunfold.db"),
            [0, 1, 2, 3, 4, 5, 6, 7],
        )?;
        let output = command.args(arguments).output()?;
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(expected));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!has_any_settings_unavailable_warning(&stderr), "{stderr}");
    }

    for (argument, expected) in [
        ("--help", ["Usage: diskunfold", "사용법:"]),
        ("--version", ["DiskUnfold ", "DiskUnfold "]),
    ] {
        let (mut command, data_dir) = launcher_with_temp_data()?;
        fs::write(
            data_dir.path().join("diskunfold.db"),
            [0, 1, 2, 3, 4, 5, 6, 7],
        )?;
        let output = command.arg(argument).output()?;
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            expected.iter().any(|text| stdout.contains(text)),
            "{stdout}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!has_any_settings_unavailable_warning(&stderr), "{stderr}");
    }

    let (mut command, data_dir) = launcher_with_temp_data()?;
    fs::write(
        data_dir.path().join("diskunfold.db"),
        [0, 1, 2, 3, 4, 5, 6, 7],
    )?;
    let output = command.arg("--help").arg("unexpected").output()?;
    assert_eq!(output.status.code(), Some(2));
    assert_localized_settings_unavailable_warning_once(&String::from_utf8_lossy(&output.stderr));

    let (mut command, data_dir) = launcher_with_temp_data()?;
    fs::write(
        data_dir.path().join("diskunfold.db"),
        [0, 1, 2, 3, 4, 5, 6, 7],
    )?;
    let output = command.args(["--lang", "fr", "--help"]).output()?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("locale.unsupported"));
    assert!(!has_any_settings_unavailable_warning(&stderr));

    let (mut command, data_dir) = launcher_with_temp_data()?;
    fs::write(
        data_dir.path().join("diskunfold.db"),
        [0, 1, 2, 3, 4, 5, 6, 7],
    )?;
    let output = command.arg("scan").arg(data_dir.path()).output()?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_localized_settings_unavailable_warning_once(&stderr);
    assert!(stderr.contains("unsupported arguments") || stderr.contains("지원하지 않는 인자"));
    Ok(())
}

#[cfg(windows)]
#[test]
fn unregistered_launcher_commands_skip_corrupt_settings_database() -> io::Result<()> {
    use std::process::Stdio;

    let corrupt_bytes = b"intentionally corrupt settings database";
    for (arguments, expected_diagnostic) in [
        (vec!["status"], None),
        (vec!["query"], None),
        (vec!["unregistered-command"], None),
        (
            vec!["--lang", "en", "delete"],
            Some("unsupported arguments"),
        ),
    ] {
        let (mut command, data_dir) = launcher_with_temp_data()?;
        let database_path = data_dir.path().join("diskunfold.db");
        fs::write(&database_path, corrupt_bytes)?;

        let output = command.args(&arguments).stdin(Stdio::null()).output()?;
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!has_any_settings_unavailable_warning(&stderr), "{stderr}");
        if let Some(expected) = expected_diagnostic {
            assert!(stderr.contains(expected), "{arguments:?}: {stderr}");
            assert_eq!(stderr, "DiskUnfold: unsupported arguments. See --help.\n");
        } else {
            assert!(
                stderr.contains("unsupported arguments") || stderr.contains("지원하지 않는 인자"),
                "{arguments:?}: {stderr}"
            );
        }
        assert_eq!(fs::read(database_path)?, corrupt_bytes, "{arguments:?}");
    }
    Ok(())
}
