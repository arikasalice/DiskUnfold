use super::{MacOsInstanceLock, MacOsInstanceLockError};
use crate::application_data::DATABASE_FILE;
use std::{
    fs,
    io::Read,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use tempfile::TempDir;

fn data_dir() -> Result<(TempDir, PathBuf), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let data = temp.path().join("DiskUnfold");
    fs::create_dir(&data)?;
    Ok((temp, data))
}

#[test]
fn lock_excludes_a_second_gui_and_releases_on_drop() -> Result<(), Box<dyn std::error::Error>> {
    let (_temp, data) = data_dir()?;
    let first = MacOsInstanceLock::acquire(&data)?;
    assert!(matches!(
        MacOsInstanceLock::acquire(&data),
        Err(MacOsInstanceLockError::Busy)
    ));
    drop(first);
    assert!(MacOsInstanceLock::acquire(&data).is_ok());
    Ok(())
}

#[test]
fn lock_rejects_a_symlink_data_directory() -> Result<(), Box<dyn std::error::Error>> {
    let (temp, data) = data_dir()?;
    let alias = temp.path().join("DiskUnfold-alias");
    symlink(&data, &alias)?;
    assert!(matches!(
        MacOsInstanceLock::acquire(&alias),
        Err(MacOsInstanceLockError::InvalidDirectory)
    ));
    Ok(())
}

#[test]
fn lock_rejects_a_symlink_sentinel_without_opening_its_target()
-> Result<(), Box<dyn std::error::Error>> {
    let (_temp, data) = data_dir()?;
    let target = data.join("outside-lock-target");
    fs::write(&target, b"unchanged")?;
    symlink(&target, data.join(".diskunfold.instance.lock"))?;
    assert!(matches!(
        MacOsInstanceLock::acquire(&data),
        Err(MacOsInstanceLockError::InvalidSentinel)
    ));
    assert_eq!(fs::read(target)?, b"unchanged");
    Ok(())
}

#[test]
fn lock_rejects_a_hard_link_sentinel() -> Result<(), Box<dyn std::error::Error>> {
    let (_temp, data) = data_dir()?;
    let source = data.join("other-file");
    fs::write(&source, b"unchanged")?;
    fs::hard_link(&source, data.join(".diskunfold.instance.lock"))?;
    assert!(matches!(
        MacOsInstanceLock::acquire(&data),
        Err(MacOsInstanceLockError::InvalidSentinel)
    ));
    assert_eq!(fs::read(source)?, b"unchanged");
    Ok(())
}

#[test]
fn replacing_the_lock_sentinel_splits_the_advisory_lock() -> Result<(), Box<dyn std::error::Error>>
{
    let (_temp, data) = data_dir()?;
    let first = MacOsInstanceLock::acquire(&data)?;
    fs::remove_file(data.join(".diskunfold.instance.lock"))?;
    let second = MacOsInstanceLock::acquire(&data)?;
    assert!(second.pin_database()?.is_none());
    drop((first, second));
    Ok(())
}

#[test]
fn sentinel_replacement_is_detected_by_path_validation() -> Result<(), Box<dyn std::error::Error>> {
    let (_temp, data) = data_dir()?;
    let lock = MacOsInstanceLock::acquire(&data)?;
    fs::remove_file(data.join(".diskunfold.instance.lock"))?;
    fs::write(data.join(".diskunfold.instance.lock"), b"replacement")?;
    assert!(matches!(
        lock.verify_sentinel_path(),
        Err(MacOsInstanceLockError::PathChanged)
    ));
    Ok(())
}

#[test]
fn database_pin_rejects_symlinks_and_hard_links() -> Result<(), Box<dyn std::error::Error>> {
    let (_temp, data) = data_dir()?;
    let target = data.join("database-target");
    fs::write(&target, b"database")?;
    symlink(&target, data.join(DATABASE_FILE))?;
    let lock = MacOsInstanceLock::acquire(&data)?;
    assert!(matches!(
        lock.pin_database(),
        Err(MacOsInstanceLockError::InvalidDatabase)
    ));
    fs::remove_file(data.join(DATABASE_FILE))?;
    fs::hard_link(&target, data.join(DATABASE_FILE))?;
    assert!(matches!(
        lock.pin_database(),
        Err(MacOsInstanceLockError::InvalidDatabase)
    ));
    Ok(())
}

#[test]
fn database_path_replacement_is_detected_while_the_original_file_is_pinned()
-> Result<(), Box<dyn std::error::Error>> {
    let (_temp, data) = data_dir()?;
    let database = data.join(DATABASE_FILE);
    fs::write(&database, b"original")?;
    let lock = MacOsInstanceLock::acquire(&data)?;
    let pinned = lock.pin_database()?.ok_or("database missing")?;
    fs::remove_file(&database)?;
    fs::write(&database, b"replacement")?;
    assert!(matches!(
        lock.verify_database_path(&pinned),
        Err(MacOsInstanceLockError::PathChanged)
    ));
    assert_eq!(fs::read(database)?, b"replacement");
    Ok(())
}

#[test]
fn data_directory_replacement_is_detected_at_a_path_checkpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, data) = data_dir()?;
    let lock = MacOsInstanceLock::acquire(&data)?;
    let moved = temp.path().join("moved-data");
    fs::rename(&data, moved)?;
    fs::create_dir(&data)?;
    assert!(matches!(
        lock.verify_data_directory_path(&data),
        Err(MacOsInstanceLockError::PathChanged)
    ));
    Ok(())
}

#[test]
fn child_directory_open_rejects_paths_and_symlinks() -> Result<(), Box<dyn std::error::Error>> {
    let (temp, data) = data_dir()?;
    let child = data.join("db-backups");
    let outside = temp.path().join("outside");
    fs::create_dir(&child)?;
    fs::create_dir(&outside)?;
    let lock = MacOsInstanceLock::acquire(&data)?;
    assert!(lock.open_child_directory("db-backups").is_ok());
    assert!(matches!(
        lock.open_child_directory("../outside"),
        Err(MacOsInstanceLockError::InvalidChildDirectory)
    ));
    fs::remove_dir(&child)?;
    symlink(&outside, &child)?;
    assert!(matches!(
        lock.open_child_directory("db-backups"),
        Err(MacOsInstanceLockError::InvalidChildDirectory)
    ));
    Ok(())
}

#[test]
fn directory_path_replacement_splits_the_lock_and_keeps_the_old_database_pinned()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, data) = data_dir()?;
    fs::write(data.join(DATABASE_FILE), b"original")?;
    let lock = MacOsInstanceLock::acquire(&data)?;
    let moved = temp.path().join("moved-data");
    fs::rename(&data, &moved)?;
    fs::create_dir(&data)?;
    fs::write(data.join(DATABASE_FILE), b"replacement")?;
    let replacement_lock = MacOsInstanceLock::acquire(&data)?;

    let mut database = lock.pin_database()?.ok_or("pinned database disappeared")?;
    let mut contents = Vec::new();
    database.read_to_end(&mut contents)?;
    assert_eq!(contents, b"original");
    assert_eq!(fs::read(data.join(DATABASE_FILE))?, b"replacement");
    drop(replacement_lock);
    Ok(())
}

#[test]
fn data_directory_must_be_absolute() -> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(
        MacOsInstanceLock::acquire(Path::new(".")),
        Err(MacOsInstanceLockError::InvalidDirectory)
    ));
    Ok(())
}
