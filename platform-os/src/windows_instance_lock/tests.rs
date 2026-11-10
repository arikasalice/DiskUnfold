use super::*;
use std::fs;

#[test]
fn independent_handles_and_same_directory_alias_exclude_each_other()
-> Result<(), Box<dyn std::error::Error>> {
    let base = tempfile::tempdir()?;
    let data = base.path().join("DataLock");
    fs::create_dir(&data)?;
    let first = WindowsInstanceLock::acquire(&data).map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        WindowsInstanceLock::acquire(&data),
        Err(InstanceLockError::Busy)
    ));
    assert!(matches!(
        WindowsInstanceLock::acquire(&data.join(".")),
        Err(InstanceLockError::Busy)
    ));
    assert!(WindowsInstanceLock::acquire(&base.path().join("dATALOCK")).is_err());
    drop(first);
    let next = WindowsInstanceLock::acquire(&data).map_err(|e| format!("{e:?}"))?;
    drop(next);
    Ok(())
}

#[test]
fn held_directory_and_sentinel_cannot_be_replaced() -> Result<(), Box<dyn std::error::Error>> {
    let base = tempfile::tempdir()?;
    let data = base.path().join("data");
    fs::create_dir(&data)?;
    let guard = WindowsInstanceLock::acquire(&data).map_err(|e| format!("{e:?}"))?;
    assert!(fs::rename(&data, base.path().join("moved")).is_err());
    assert!(fs::remove_file(data.join(".diskunfold.instance.lock")).is_err());
    drop(guard);
    fs::rename(&data, base.path().join("moved"))?;
    Ok(())
}

#[test]
fn existing_hardlinked_sentinel_is_not_trusted() -> Result<(), Box<dyn std::error::Error>> {
    let base = tempfile::tempdir()?;
    let data = base.path().join("data");
    fs::create_dir(&data)?;
    let source = base.path().join("other");
    fs::write(&source, b"preserve")?;
    fs::hard_link(&source, data.join(".diskunfold.instance.lock"))?;
    assert!(matches!(
        WindowsInstanceLock::acquire(&data),
        Err(InstanceLockError::InvalidSentinel)
    ));
    assert_eq!(fs::read(&source)?, b"preserve");
    Ok(())
}

#[test]
fn database_pin_denies_replacement_but_allows_writes() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    let guard = WindowsInstanceLock::acquire(dir.path()).map_err(|error| format!("{error:?}"))?;
    assert!(
        guard
            .pin_database()
            .map_err(|error| format!("{error:?}"))?
            .is_none()
    );
    fs::write(&path, b"first")?;
    let pin = guard
        .pin_database()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing pin")?;
    fs::write(&path, b"second")?;
    assert!(fs::rename(&path, dir.path().join("replaced.db")).is_err());
    drop(pin);
    fs::rename(&path, dir.path().join("replaced.db"))?;
    Ok(())
}
