use super::*;
use du_user_env::i18n::embedded;
use std::fs;

fn scan(arguments: &[&str]) -> Result<Scan, String> {
    let parsed = crate::args::parse(
        std::iter::once("scan")
            .chain(arguments.iter().copied())
            .map(OsString::from),
    )
    .ok_or("no arguments")?;
    match parsed.command {
        crate::args::Command::Scan(scan) => Ok(scan),
        _ => Err("not a scan".into()),
    }
}

#[test]
fn a_draft_comes_from_the_command_line_with_absolute_paths_and_its_exclusions() -> Result<(), String>
{
    let folder = tempfile::tempdir().map_err(|error| error.to_string())?;
    let root = folder.path().to_string_lossy().into_owned();
    let excluded = folder.path().join("cache").to_string_lossy().into_owned();
    let built = draft(&scan(&[
        &root,
        "--exclude",
        &excluded,
        "--exclude",
        "*.tmp",
    ])?)?;
    assert_eq!(built.source, ScopeSource::Cli);
    assert_eq!(built.includes.len(), 1);
    assert_eq!(built.includes[0].display, root);
    assert!(
        matches!(&built.exclusions[0], ScopeExclusion::Path { path } if path.display == excluded)
    );
    assert!(
        matches!(&built.exclusions[1], ScopeExclusion::NamePattern { pattern } if pattern == "*.tmp")
    );
    // A relative path is taken from the current folder.
    let relative = draft(&scan(&["relative-folder"])?)?;
    assert!(std::path::Path::new(&relative.includes[0].display).is_absolute());
    Ok(())
}

/// Runs a scan of a real folder to its end on this platform.
#[test]
fn a_scan_of_a_real_folder_reports_its_end_and_warns_that_history_is_not_recorded()
-> Result<(), Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    fs::create_dir(folder.path().join("inner"))?;
    fs::write(folder.path().join("inner").join("one.bin"), [1_u8; 7])?;
    fs::write(folder.path().join("two.bin"), [2_u8; 5])?;
    let registry = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let message = Messages::new(&registry, "en");
    let root = folder.path().to_string_lossy().into_owned();
    let scan = scan(&["--format", "json", &root])?;
    let engine = engine()?;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let exit = run(
        &engine,
        &scan,
        scan.format(),
        &message,
        &mut stdout,
        &mut stderr,
    );
    let errors = String::from_utf8_lossy(&stderr);
    assert_eq!(exit, ExitCode::SUCCESS, "{errors}");
    let mut lines = errors.lines();
    assert!(
        lines
            .next()
            .is_some_and(|line| line.starts_with("DiskUnfold: Scan accepted: ")),
        "{errors}"
    );
    assert_eq!(lines.next(), Some("Not recorded in history."), "{errors}");
    let summary: Value = serde_json::from_slice(&stdout)?;
    assert_eq!(summary["command"], "scan");
    assert_eq!(summary["result"]["state"], "done");
    // The root, the folder inside it and the two files.
    assert_eq!(summary["result"]["completed"], 4);
    assert_eq!(summary["result"]["failed"], 0);
    assert!(summary["result"]["resultVersion"].is_string());
    Ok(())
}

#[test]
fn a_path_that_does_not_exist_is_a_usage_error_with_an_empty_stdout()
-> Result<(), Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let missing = folder.path().join("missing").to_string_lossy().into_owned();
    let registry = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let message = Messages::new(&registry, "en");
    let scan = scan(&[&missing])?;
    let engine = engine()?;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let exit = run(
        &engine,
        &scan,
        scan.format(),
        &message,
        &mut stdout,
        &mut stderr,
    );
    assert!(stdout.is_empty());
    let errors = String::from_utf8_lossy(&stderr);
    assert_eq!(errors.lines().count(), 1, "{errors}");
    assert_ne!(exit, ExitCode::SUCCESS, "{errors}");
    Ok(())
}
