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

/// The sandbox folder that plays the role of an Applications folder.
fn folders(root: &Path) -> Vec<PathBuf> {
    vec![root.to_path_buf()]
}

#[test]
fn creates_the_folder_and_a_link_that_resolves_to_the_bundle_executable() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let executable = bundle(sandbox.path(), "DiskUnfold")?;
    let directory = sandbox.path().join("home/.local/bin");

    assert_eq!(
        install_link(&executable, &directory, &folders(sandbox.path()))?,
        LinkOutcome::Created
    );

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
    let allowed = folders(sandbox.path());
    install_link(&executable, &directory, &allowed)?;
    assert_eq!(
        install_link(&executable, &directory, &allowed)?,
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
    let allowed = folders(sandbox.path());
    install_link(&old, &directory, &allowed)?;

    assert_eq!(
        install_link(&new, &directory, &allowed)?,
        LinkOutcome::Updated
    );

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
    let allowed = folders(sandbox.path());

    let file_directory = sandbox.path().join("file-bin");
    fs::create_dir(&file_directory)?;
    fs::write(file_directory.join(LINK_NAME), b"user script")?;
    assert!(matches!(
        install_link(&executable, &file_directory, &allowed),
        Err(LinkError::TargetOccupied(_))
    ));
    assert_eq!(fs::read(file_directory.join(LINK_NAME))?, b"user script");

    let link_directory = sandbox.path().join("link-bin");
    fs::create_dir(&link_directory)?;
    let foreign = sandbox.path().join("other-tool");
    fs::write(&foreign, b"tool")?;
    symlink(&foreign, link_directory.join(LINK_NAME))?;
    assert!(matches!(
        install_link(&executable, &link_directory, &allowed),
        Err(LinkError::TargetOccupied(_))
    ));
    assert_eq!(fs::read_link(link_directory.join(LINK_NAME))?, foreign);

    let folder_directory = sandbox.path().join("folder-bin");
    fs::create_dir_all(folder_directory.join(LINK_NAME))?;
    assert!(matches!(
        install_link(&executable, &folder_directory, &allowed),
        Err(LinkError::TargetOccupied(_))
    ));
    Ok(())
}

#[test]
fn only_an_application_bundle_executable_is_linked() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let directory = sandbox.path().join("bin");
    let allowed = folders(sandbox.path());
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
    ] {
        assert!(
            matches!(
                install_link(&executable, &directory, &allowed),
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
fn a_bundle_outside_the_application_folders_is_refused() -> TestResult {
    // A scratch copy, a disk image mount and a quarantine copy all sit outside the
    // allow-list, whatever their names; the folder name alone grants nothing.
    let sandbox = tempfile::tempdir()?;
    let allowed_root = sandbox.path().join("Applications");
    fs::create_dir(&allowed_root)?;
    let directory = sandbox.path().join("bin");
    for outside in [
        "scratch",
        "Volumes/DiskUnfold",
        "private/var/AppTranslocation/X/d",
        "Applications-copy",
        "Applications/../elsewhere",
    ] {
        let executable = bundle(&sandbox.path().join(outside), "DiskUnfold")?;
        assert!(
            matches!(
                install_link(&executable, &directory, &folders(&allowed_root)),
                Err(LinkError::NotInstalledBundle)
            ),
            "{outside}"
        );
    }
    assert!(!directory.exists());
    Ok(())
}

#[test]
fn a_bundle_in_either_application_folder_is_linked() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let home = sandbox.path().join("home");
    let system = sandbox.path().join("system-Applications");
    fs::create_dir_all(&system)?;
    let allowed = vec![system.clone(), home.join(USER_APPLICATIONS)];
    for root in &allowed {
        let executable = bundle(root, "DiskUnfold")?;
        let directory = sandbox.path().join("bin");
        let outcome = install_link(&executable, &directory, &allowed)?;
        assert!(matches!(
            outcome,
            LinkOutcome::Created | LinkOutcome::Updated
        ));
        assert_eq!(
            fs::canonicalize(directory.join(LINK_NAME))?,
            fs::canonicalize(&executable)?
        );
    }
    Ok(())
}

#[test]
fn a_link_inside_an_application_folder_cannot_lead_outside_it() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let applications = sandbox.path().join("Applications");
    fs::create_dir(&applications)?;
    bundle(&sandbox.path().join("tmp"), "DiskUnfold")?;
    // `Applications/DiskUnfold.app` is a link to a bundle that lives elsewhere.
    symlink(
        sandbox.path().join("tmp/DiskUnfold.app"),
        applications.join("DiskUnfold.app"),
    )?;
    let through_link = applications
        .join("DiskUnfold.app/Contents/MacOS")
        .join(LINK_NAME);
    assert!(through_link.exists());
    let directory = sandbox.path().join("bin");
    assert!(matches!(
        install_link(&through_link, &directory, &folders(&applications)),
        Err(LinkError::NotInstalledBundle)
    ));
    assert!(!directory.exists());
    Ok(())
}

#[test]
fn the_application_folders_are_the_system_and_the_user_folder() {
    assert_eq!(
        application_folders(Path::new("/Users/someone")),
        [
            PathBuf::from("/Applications"),
            PathBuf::from("/Users/someone/Applications")
        ]
    );
}

#[test]
fn an_uncreatable_folder_reports_the_io_error() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let executable = bundle(sandbox.path(), "DiskUnfold")?;
    let blocker = sandbox.path().join("blocker");
    fs::write(&blocker, b"a file, not a folder")?;
    assert!(matches!(
        install_link(&executable, &blocker.join("bin"), &folders(sandbox.path())),
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
fn the_dialog_language_follows_the_primary_language_tag() {
    assert_eq!(Language::from_tag("ko"), Language::Korean);
    assert_eq!(Language::from_tag("ko-KR"), Language::Korean);
    assert_eq!(Language::from_tag("ko_KR"), Language::Korean);
    assert_eq!(Language::from_tag("en-US"), Language::English);
    assert_eq!(Language::from_tag("ja"), Language::English);
    assert_eq!(Language::from_tag(""), Language::English);
}

#[test]
fn the_result_text_names_the_link_and_the_path_step() {
    let directory = Path::new("/Users/someone/.local/bin");
    let english = Language::English;
    let (title, detail) = describe(&Ok(LinkOutcome::Created), directory, english);
    assert_eq!(title, "Command line tool installed");
    assert!(detail.contains("/Users/someone/.local/bin/diskunfold"));
    assert!(detail.contains("export PATH="));
    let (title, _) = describe(&Ok(LinkOutcome::AlreadyLinked), directory, english);
    assert_eq!(title, "The command line tool is already installed");
    let (title, detail) = describe(&Err(LinkError::NotInstalledBundle), directory, english);
    assert_eq!(title, "Move DiskUnfold to Applications first");
    assert!(detail.contains("Applications"));
    let occupied = LinkError::TargetOccupied(directory.join(LINK_NAME));
    assert!(
        describe(&Err(occupied), directory, english)
            .1
            .contains("was not changed")
    );
}

#[test]
fn every_result_has_korean_text_that_differs_from_english() {
    let directory = Path::new("/Users/someone/.local/bin");
    let results = [
        Ok(LinkOutcome::Created),
        Ok(LinkOutcome::AlreadyLinked),
        Err(LinkError::NotInstalledBundle),
        Err(LinkError::TargetOccupied(directory.join(LINK_NAME))),
        Err(LinkError::Io(io::Error::other("denied"))),
    ];
    for result in &results {
        let english = describe(result, directory, Language::English);
        let korean = describe(result, directory, Language::Korean);
        assert_ne!(english, korean, "{result:?}");
        assert!(
            korean
                .0
                .chars()
                .any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c)),
            "{result:?}"
        );
    }
    let (_, detail) = describe(
        &Err(LinkError::NotInstalledBundle),
        directory,
        Language::Korean,
    );
    assert!(detail.contains("Applications"));
    assert_ne!(
        describe_missing_home(Language::Korean),
        describe_missing_home(Language::English)
    );
}
