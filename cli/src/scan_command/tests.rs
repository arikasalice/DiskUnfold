use super::*;
use du_user_env::i18n::embedded;
use std::fs;

struct ScanRun {
    exit: ExitCode,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

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

/// Runs a scan with and without a database to compare its unaffected result.
#[test]
fn a_real_scan_records_history_only_when_the_current_database_exists()
-> Result<(), Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let root_dir = folder.path().join("scan-root");
    let data_directory = folder.path().join("data");
    fs::create_dir(&root_dir)?;
    fs::create_dir(&data_directory)?;
    fs::create_dir(root_dir.join("inner"))?;
    fs::write(root_dir.join("inner").join("one.bin"), [1_u8; 7])?;
    fs::write(root_dir.join("two.bin"), [2_u8; 5])?;
    let registry = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let message = Messages::new(&registry, "en");
    // Scope review does not follow links in a selected path, and the macOS
    // temporary folder sits under /var, a link to /private/var.
    #[cfg(not(windows))]
    let base = fs::canonicalize(&root_dir)?;
    #[cfg(windows)]
    let base = root_dir;
    let root = base.to_string_lossy().into_owned();
    let scan = scan(&["--format", "json", &root])?;
    let missing = run_real_scan(&scan, &message, &data_directory)?;
    assert_eq!(missing.exit, ExitCode::SUCCESS);
    assert!(String::from_utf8_lossy(&missing.stderr).contains("history.not_recorded"));

    let database_path = data_directory.join(du_platform_os::application_data::DATABASE_FILE);
    drop(
        du_app_db::AppDb::create_gui_with_history(&database_path)
            .map_err(|error| format!("{error:?}"))?,
    );
    let recorded = run_real_scan(&scan, &message, &data_directory)?;
    assert_eq!(recorded.exit, ExitCode::SUCCESS);
    assert!(!String::from_utf8_lossy(&recorded.stderr).contains("history.not_recorded"));

    let mut missing_summary: Value = serde_json::from_slice(&missing.stdout)?;
    let mut recorded_summary: Value = serde_json::from_slice(&recorded.stdout)?;
    assert!(recorded_summary["result"]["resultVersion"].is_string());
    for summary in [&mut missing_summary, &mut recorded_summary] {
        summary["result"]
            .as_object_mut()
            .ok_or("result object")?
            .remove("jobRef");
        summary["result"]
            .as_object_mut()
            .ok_or("result object")?
            .remove("resultVersion");
    }
    assert_eq!(missing_summary, recorded_summary);
    assert_eq!(recorded_summary["command"], "scan");
    assert_eq!(recorded_summary["result"]["state"], "done");
    assert_eq!(recorded_summary["result"]["completed"], 4);
    assert_eq!(recorded_summary["result"]["failed"], 0);
    let mut database = du_app_db::AppDb::open_read_only_live(&database_path)
        .map_err(|error| format!("{error:?}"))?;
    let page = database
        .read_history_page(du_app_db::history_read::Filter::default(), None, 10)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(page.rows.len(), 1);
    Ok(())
}

fn run_real_scan(
    scan: &Scan,
    message: &Messages<'_>,
    data_directory: &std::path::Path,
) -> Result<ScanRun, Box<dyn std::error::Error>> {
    let (engine, history) = engine()?;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let exit = run_with_data_directory(
        &engine,
        HistoryRun {
            history: &history,
            data_directory: Some(data_directory.to_path_buf()),
        },
        scan,
        scan.format(),
        message,
        &mut stdout,
        &mut stderr,
    );
    Ok(ScanRun {
        exit,
        stdout,
        stderr,
    })
}

#[test]
fn a_path_that_does_not_exist_is_a_usage_error_with_an_empty_stdout()
-> Result<(), Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let missing = folder.path().join("missing").to_string_lossy().into_owned();
    let registry = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let message = Messages::new(&registry, "en");
    let scan = scan(&[&missing])?;
    let (engine, history) = engine()?;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let exit = run(
        &engine,
        &history,
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

#[cfg(unix)]
#[test]
fn a_path_through_a_symbolic_link_explains_the_resolved_scan_path()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let folder = tempfile::tempdir()?;
    let base = fs::canonicalize(folder.path())?;
    let target = base.join("real");
    fs::create_dir_all(target.join("x"))?;
    let link = base.join("link");
    symlink(&target, &link)?;
    let root = link.join("x");
    let registry = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let message = Messages::new(&registry, "en");
    let scan = scan(&[&root.to_string_lossy()])?;
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
    assert_eq!(exit, ExitCode::from(2), "{errors}");
    assert!(stdout.is_empty());
    assert!(errors.contains("path.unresolvable"), "{errors}");
    assert!(
        errors.contains(&link.to_string_lossy().to_string()),
        "{errors}"
    );
    assert!(
        errors.contains(&target.to_string_lossy().to_string()),
        "{errors}"
    );
    assert!(
        errors.contains(&root.to_string_lossy().to_string()),
        "{errors}"
    );
    Ok(())
}
