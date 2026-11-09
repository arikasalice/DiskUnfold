use std::{io, process::Command};

#[test]
fn launcher_prints_help_and_version_to_stdout() -> io::Result<()> {
    for (flag, expected) in [
        ("--help", "Usage: diskunfold"),
        (
            "--version",
            concat!("DiskUnfold ", env!("CARGO_PKG_VERSION")),
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
            .arg(flag)
            .output()?;
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(expected));
        assert!(output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn launcher_rejects_unsupported_commands_and_trailing_arguments() -> io::Result<()> {
    for args in [&["scan"][..], &["--version", "extra"][..]] {
        let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
            .args(args)
            .output()?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported arguments"));
    }
    Ok(())
}
