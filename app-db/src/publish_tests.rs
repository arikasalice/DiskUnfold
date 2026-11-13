//! Marker-route publication: FR3.HD1.app-db.DD1 interruption at every step.

use std::{fs, io, path::Path};

use du_core_contracts::settings::SettingsStore;

use crate::{
    AppDb, CreationRecovery, OpenError, initialize_settings,
    publish::{Interrupt, Step, create_gui_published, is_incomplete},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const STEPS: [Step; 5] = [
    Step::SentinelWritten,
    Step::DatabaseCreated,
    Step::DatabaseCopied,
    Step::MarkerWritten,
    Step::SentinelRemoved,
];

fn no_hard_links(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

fn create_without_links(
    path: &Path,
    hook: impl FnMut(Step) -> Result<(), Interrupt>,
) -> Result<AppDb, OpenError> {
    create_gui_published(
        path,
        initialize_settings,
        || Ok(()),
        AppDb::open_gui,
        no_hard_links,
        hook,
    )
}

/// Names in `dir`, ignoring temporary creation files that a kill leaves behind.
fn names(dir: &Path) -> Result<Vec<String>, io::Error> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".tmp") {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn crash_at(at: Step) -> impl FnMut(Step) -> Result<(), Interrupt> {
    move |step| {
        if step == at {
            Err(Interrupt::Crash)
        } else {
            Ok(())
        }
    }
}

fn assert_usable(path: &Path) -> TestResult {
    let mut db = AppDb::open_gui(path).map_err(|e| format!("{e:?}"))?;
    assert_eq!(db.load().map_err(|e| format!("{e:?}"))?.revision, 0);
    Ok(())
}

#[test]
fn marker_route_publishes_a_complete_database_with_its_marker() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    drop(create_without_links(&path, |_| Ok(())).map_err(|e| format!("{e:?}"))?);
    assert_eq!(
        names(dir.path())?,
        ["diskunfold.db", "diskunfold.db.created"]
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("diskunfold.db.created"))?,
        format!("{}\n", fs::metadata(&path)?.len())
    );
    assert!(!is_incomplete(&path)?);
    assert_eq!(
        AppDb::recover_incomplete_creation(&path)?,
        CreationRecovery::NotApplicable
    );
    assert_usable(&path)
}

#[test]
fn hard_link_route_leaves_no_marker_and_stays_valid() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    assert_eq!(names(dir.path())?, ["diskunfold.db"]);
    assert_eq!(
        AppDb::recover_incomplete_creation(&path)?,
        CreationRecovery::NotApplicable
    );
    assert_usable(&path)
}

#[test]
fn other_link_failures_do_not_use_the_marker_route() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    let result = create_gui_published(
        &path,
        initialize_settings,
        || Ok(()),
        AppDb::open_gui,
        |_, _| Err(io::Error::other("disk on fire")),
        |_| Ok(()),
    );
    assert!(matches!(result, Err(OpenError::Io(_))));
    assert!(fs::read_dir(dir.path())?.next().is_none());
    Ok(())
}

#[test]
fn killed_creation_is_never_opened_and_is_recovered_at_every_step() -> TestResult {
    for at in STEPS {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("diskunfold.db");
        let result = create_without_links(&path, crash_at(at));
        assert!(matches!(result, Err(OpenError::Io(_))), "{at:?}");
        let before: Vec<_> = names(dir.path())?;
        let finished = matches!(at, Step::MarkerWritten | Step::SentinelRemoved);
        // Every consumer refuses an interrupted creation and changes nothing.
        if !finished {
            for open in [
                AppDb::open_gui,
                AppDb::open_read_only,
                AppDb::open_read_only_live,
            ] {
                assert!(
                    matches!(open(&path), Err(OpenError::IncompleteCreation)),
                    "{at:?}"
                );
            }
            assert!(matches!(
                AppDb::probe_schema_version_read_only(&path),
                Err(OpenError::IncompleteCreation)
            ));
            assert_eq!(names(dir.path())?, before, "{at:?}");
        }
        let expected = match at {
            Step::SentinelWritten | Step::DatabaseCreated | Step::DatabaseCopied => {
                CreationRecovery::Discarded
            }
            Step::MarkerWritten => CreationRecovery::Completed,
            Step::SentinelRemoved => CreationRecovery::NotApplicable,
        };
        assert_eq!(
            AppDb::recover_incomplete_creation(&path)?,
            expected,
            "{at:?}"
        );
        if finished {
            assert_usable(&path)?;
        } else {
            assert!(names(dir.path())?.is_empty(), "{at:?}");
            drop(create_without_links(&path, |_| Ok(())).map_err(|e| format!("{e:?}"))?);
            assert_usable(&path)?;
        }
        assert!(!is_incomplete(&path)?, "{at:?}");
    }
    Ok(())
}

#[test]
fn survivable_failures_remove_what_the_call_created() -> TestResult {
    for at in &STEPS[..4] {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("diskunfold.db");
        let at = *at;
        let result = create_without_links(&path, |step| {
            if step == at {
                Err(Interrupt::Fail(io::Error::other("injected")))
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(OpenError::Io(_))), "{at:?}");
        assert!(fs::read_dir(dir.path())?.next().is_none(), "{at:?}");
        drop(create_without_links(&path, |_| Ok(())).map_err(|e| format!("{e:?}"))?);
        assert_usable(&path)?;
    }
    Ok(())
}

#[test]
fn a_truncated_copy_or_a_wrong_marker_is_discarded() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    let good = dir.path().join("good.db");
    drop(AppDb::create_gui(&good).map_err(|e| format!("{e:?}"))?);
    let bytes = fs::read(&good)?;
    fs::remove_file(&good)?;
    for marker in [
        None,
        Some("1\n".to_owned()),
        Some(format!("{}", bytes.len())),
    ] {
        fs::write(&path, &bytes[..bytes.len() / 2])?;
        fs::write(dir.path().join("diskunfold.db.creating"), b"")?;
        if let Some(text) = &marker {
            fs::write(dir.path().join("diskunfold.db.created"), text)?;
        }
        assert!(is_incomplete(&path)?);
        assert_eq!(
            AppDb::recover_incomplete_creation(&path)?,
            CreationRecovery::Discarded
        );
        assert!(names(dir.path())?.is_empty());
    }
    Ok(())
}

#[test]
fn existing_database_wins_and_the_marker_route_removes_only_its_own_files() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let before = fs::read(&path)?;
    let result = create_without_links(&path, |_| Ok(()));
    assert!(
        matches!(result, Err(OpenError::Io(ref e)) if e.kind() == io::ErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read(&path)?, before);
    assert_eq!(names(dir.path())?, ["diskunfold.db"]);
    Ok(())
}

#[test]
fn a_second_creator_does_not_disturb_a_running_marker_creation() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    let mut raced = false;
    let result = create_without_links(&path, |step| {
        if step == Step::DatabaseCopied && !raced {
            raced = true;
            let other = create_without_links(&path, |_| Ok(()));
            if !matches!(other, Err(OpenError::Io(ref e)) if e.kind() == io::ErrorKind::AlreadyExists)
            {
                return Err(Interrupt::Fail(io::Error::other(
                    "second creator did not back off",
                )));
            }
        }
        Ok(())
    });
    drop(result.map_err(|e| format!("{e:?}"))?);
    assert!(raced);
    assert_eq!(
        names(dir.path())?,
        ["diskunfold.db", "diskunfold.db.created"]
    );
    assert_usable(&path)
}
