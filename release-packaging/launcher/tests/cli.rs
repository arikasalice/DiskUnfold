use std::{
    io::{self, Read},
    process::Command,
};

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
        let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
            .args(args)
            .output()?;
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
        assert!(output.stderr.is_empty());
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn help_and_version_return_exit_five_when_stdout_is_closed() -> io::Result<()> {
    use std::process::Stdio;

    for flag in ["--help", "--version"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
            .args(["--lang", "en", flag])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        drop(child.stdout.take());
        let mut stderr = Vec::new();
        child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("stderr pipe missing"))?
            .read_to_end(&mut stderr)?;
        let status = child.wait()?;
        assert_eq!(status.code(), Some(5), "{flag}");
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
        .args(["--lang", "en", "--help"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    drop(child.stdout.take());
    drop(child.stderr.take());
    assert_eq!(child.wait()?.code(), Some(5));
    Ok(())
}

#[test]
fn launcher_rejects_unsupported_commands_and_trailing_arguments() -> io::Result<()> {
    for args in [&["scan"][..], &["--version", "extra"][..]] {
        let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
            .args(["--lang", "en"])
            .args(args)
            .output()?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported arguments"));
    }
    Ok(())
}

#[test]
fn launcher_applies_the_language_option_on_either_side_of_help() -> io::Result<()> {
    for args in [["--lang", "KO", "--help"], ["--help", "--lang", "ko-KR"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
            .args(args)
            .output()?;
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("사용법: diskunfold"));
        assert!(output.stderr.is_empty());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
        .args(["--lang", "fr", "--help"])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("locale.unsupported"));
    Ok(())
}

#[cfg(windows)]
#[test]
fn help_and_version_skip_a_corrupt_settings_database() -> io::Result<()> {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let data_dir = std::env::temp_dir().join(format!(
        "diskunfold-cli-help-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&data_dir)?;
    fs::write(data_dir.join("diskunfold.db"), [0, 1, 2, 3, 4, 5, 6, 7])?;

    for (arguments, expected) in [
        (vec!["--lang", "en", "--help"], "Usage: diskunfold"),
        (vec!["--lang", "ko", "--version"], "DiskUnfold "),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
            .args(arguments)
            .env("DISKUNFOLD_DATA_DIR", &data_dir)
            .output()?;
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(expected));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("settings_unavailable"), "{stderr}");
    }

    for (argument, expected) in [
        ("--help", ["Usage: diskunfold", "사용법:"]),
        ("--version", ["DiskUnfold ", "DiskUnfold "]),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
            .arg(argument)
            .env("DISKUNFOLD_DATA_DIR", &data_dir)
            .output()?;
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            expected.iter().any(|text| stdout.contains(text)),
            "{stdout}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("settings_unavailable"), "{stderr}");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
        .arg("--help")
        .arg("unexpected")
        .env("DISKUNFOLD_DATA_DIR", &data_dir)
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("settings_unavailable"));

    let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
        .args(["--lang", "fr", "--help"])
        .env("DISKUNFOLD_DATA_DIR", &data_dir)
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("locale.unsupported"));
    assert!(!stderr.contains("settings_unavailable"));

    let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
        .arg("scan")
        .arg(&data_dir)
        .env("DISKUNFOLD_DATA_DIR", &data_dir)
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("settings_unavailable"));
    assert!(stderr.contains("unsupported arguments") || stderr.contains("지원하지 않는 인자"));

    fs::remove_dir_all(data_dir)?;
    Ok(())
}
