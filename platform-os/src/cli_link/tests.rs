use super::*;
use std::os::unix::fs::PermissionsExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Creates `<root>/<name>.app/Contents/MacOS/diskunfold` as an executable file.
fn bundle(root: &Path, name: &str) -> io::Result<PathBuf> {
    let dir = root.join(format!("{name}.app/Contents/MacOS"));
    fs::create_dir_all(&dir)?;
    let executable = dir.join(LINK_NAME);
    fs::write(&executable, b"#!/bin/sh\n")?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
    Ok(executable)
}

#[test]
fn creates_the_folder_and_a_link_that_resolves_to_the_bundle_executable() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let executable = bundle(sandbox.path(), "DiskUnfold")?;
    let directory = sandbox.path().join("home/.local/bin");

    assert_eq!(install_link(&executable, &directory)?, LinkOutcome::Created);

    let link = directory.join(LINK_NAME);
    assert_eq!(fs::read_link(&link)?, executable);
    assert_eq!(fs::canonicalize(&link)?, fs::canonicalize(&executable)?);
    Ok(())
}

#[test]
fn a_second_install_changes_nothing() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let executable = bundle(sandbox.path(), "DiskUnfold")?;
    let directory = sandbox.path().join("bin");
    install_link(&executable, &directory)?;
    assert_eq!(
        install_link(&executable, &directory)?,
        LinkOutcome::AlreadyLinked
    );
    assert_eq!(fs::read_link(directory.join(LINK_NAME))?, executable);
    Ok(())
}

#[test]
fn a_link_to_an_earlier_bundle_location_is_replaced_without_leaving_a_staging_link() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let old = bundle(&sandbox.path().join("old"), "DiskUnfold")?;
    let new = bundle(&sandbox.path().join("new"), "DiskUnfold")?;
    let directory = sandbox.path().join("bin");
    install_link(&old, &directory)?;

    assert_eq!(install_link(&new, &directory)?, LinkOutcome::Updated);

    assert_eq!(fs::read_link(directory.join(LINK_NAME))?, new);
    let names: Vec<_> = fs::read_dir(&directory)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<_, _>>()?;
    assert_eq!(names, [OsStr::new(LINK_NAME)]);
    Ok(())
}

#[test]
fn an_existing_file_or_foreign_link_is_never_overwritten() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let executable = bundle(sandbox.path(), "DiskUnfold")?;

    let file_directory = sandbox.path().join("file-bin");
    fs::create_dir(&file_directory)?;
    fs::write(file_directory.join(LINK_NAME), b"user script")?;
    assert!(matches!(
        install_link(&executable, &file_directory),
        Err(LinkError::TargetOccupied(_))
    ));
    assert_eq!(fs::read(file_directory.join(LINK_NAME))?, b"user script");

    let link_directory = sandbox.path().join("link-bin");
    fs::create_dir(&link_directory)?;
    let foreign = sandbox.path().join("other-tool");
    fs::write(&foreign, b"tool")?;
    symlink(&foreign, link_directory.join(LINK_NAME))?;
    assert!(matches!(
        install_link(&executable, &link_directory),
        Err(LinkError::TargetOccupied(_))
    ));
    assert_eq!(fs::read_link(link_directory.join(LINK_NAME))?, foreign);

    let folder_directory = sandbox.path().join("folder-bin");
    fs::create_dir_all(folder_directory.join(LINK_NAME))?;
    assert!(matches!(
        install_link(&executable, &folder_directory),
        Err(LinkError::TargetOccupied(_))
    ));
    Ok(())
}

#[test]
fn only_an_installed_application_bundle_executable_is_linked() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let directory = sandbox.path().join("bin");
    let loose = sandbox.path().join(LINK_NAME);
    fs::write(&loose, b"x")?;
    let wrong_name = bundle(sandbox.path(), "DiskUnfold")?.with_file_name("other");
    fs::write(&wrong_name, b"x")?;
    let no_app_suffix = sandbox.path().join("Plain/Contents/MacOS");
    fs::create_dir_all(&no_app_suffix)?;
    fs::write(no_app_suffix.join(LINK_NAME), b"x")?;
    let missing = sandbox
        .path()
        .join("Gone.app/Contents/MacOS")
        .join(LINK_NAME);

    for executable in [
        loose,
        wrong_name,
        no_app_suffix.join(LINK_NAME),
        missing,
        PathBuf::from("DiskUnfold.app/Contents/MacOS/diskunfold"),
        PathBuf::from("/Volumes/DiskUnfold/DiskUnfold.app/Contents/MacOS/diskunfold"),
        PathBuf::from("/private/var/AppTranslocation/X/d/DiskUnfold.app/Contents/MacOS/diskunfold"),
    ] {
        assert!(
            matches!(
                install_link(&executable, &directory),
                Err(LinkError::NotInstalledBundle)
            ),
            "{}",
            executable.display()
        );
    }
    // Nothing was created for a rejected executable.
    assert!(!directory.exists());
    Ok(())
}

#[test]
fn an_uncreatable_folder_reports_the_io_error() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let executable = bundle(sandbox.path(), "DiskUnfold")?;
    let blocker = sandbox.path().join("blocker");
    fs::write(&blocker, b"a file, not a folder")?;
    assert!(matches!(
        install_link(&executable, &blocker.join("bin")),
        Err(LinkError::Io(_))
    ));
    Ok(())
}

#[test]
fn the_default_folder_is_under_the_home_directory() {
    assert_eq!(
        default_directory(Path::new("/Users/someone")),
        Path::new("/Users/someone/.local/bin")
    );
}

#[test]
fn the_result_text_names_the_link_and_the_path_step() {
    let directory = Path::new("/Users/someone/.local/bin");
    let (title, detail) = describe(&Ok(LinkOutcome::Created), directory);
    assert_eq!(title, "Command line tool installed");
    assert!(detail.contains("/Users/someone/.local/bin/diskunfold"));
    assert!(detail.contains("export PATH="));
    let (title, _) = describe(&Ok(LinkOutcome::AlreadyLinked), directory);
    assert_eq!(title, "The command line tool is already installed");
    let (title, detail) = describe(&Err(LinkError::NotInstalledBundle), directory);
    assert_eq!(title, "Install DiskUnfold first");
    assert!(detail.contains("Applications"));
    let occupied = LinkError::TargetOccupied(directory.join(LINK_NAME));
    assert!(
        describe(&Err(occupied), directory)
            .1
            .contains("was not changed")
    );
}
