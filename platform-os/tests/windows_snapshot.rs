#![cfg(windows)]
use du_core_contracts::{Id, snapshot_spool::CaptureSpool};
use du_platform_os::windows_snapshot::SnapshotDirectory;
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    sync::{Arc, Barrier},
    thread,
};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn temp_path(root: &std::path::Path, value: &Id) -> std::path::PathBuf {
    root.join(format!(".tmp-{}.dusnap.part", value.as_str()))
}

#[test]
fn exclusive_staging_blocks_cleaners_and_removes_unregistered_file()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = SnapshotDirectory::open(dir.path()).map_err(|error| format!("{error:?}"))?;
    let temp = id(1)?;
    let path = temp_path(dir.path(), &temp);
    let mut writer = root.create(&temp)?;
    writer.write_all(b"staged")?;
    writer.sync()?;
    assert!(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .is_err()
    );
    assert!(fs::remove_file(&path).is_err());
    assert!(root.create(&temp).is_err());
    writer.seek(SeekFrom::Start(0))?;
    let mut contents = Vec::new();
    writer.read_to_end(&mut contents)?;
    assert_eq!(contents, b"staged");
    drop(writer);
    assert!(!path.exists());
    Ok(())
}

#[test]
fn concurrent_creates_for_same_temporary_name_have_one_winner()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = SnapshotDirectory::open(dir.path()).map_err(|error| format!("{error:?}"))?;
    let start = Arc::new(Barrier::new(2));
    let hold = Arc::new(Barrier::new(2));
    let temp = id(2)?;
    thread::scope(|scope| {
        let spawn = |start: Arc<Barrier>, hold: Arc<Barrier>, temp: Id| {
            let root = &root;
            scope.spawn(move || {
                start.wait();
                let writer = root.create(&temp);
                let won = writer.is_ok();
                hold.wait();
                won
            })
        };
        let a = spawn(start.clone(), hold.clone(), temp.clone());
        let b = spawn(start, hold, temp.clone());
        assert_ne!(
            a.join().map_err(|_| "first panic")?,
            b.join().map_err(|_| "second panic")?
        );
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    assert!(!temp_path(dir.path(), &temp).exists());
    Ok(())
}

#[test]
fn verified_ancestors_cannot_be_renamed_while_staging() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let parent = temp.path().join("parent");
    let snapshots = parent.join("snapshots");
    fs::create_dir_all(&snapshots)?;
    let root = SnapshotDirectory::open(&snapshots).map_err(|error| format!("{error:?}"))?;
    let mut writer = root.create(&id(30)?)?;
    writer.write_all(b"anchored")?;
    assert!(fs::rename(&parent, temp.path().join("moved")).is_err());
    assert!(fs::rename(&snapshots, parent.join("other")).is_err());
    drop(writer);
    drop(root);
    fs::rename(&parent, temp.path().join("moved"))?;
    Ok(())
}

#[test]
fn rejecting_reparse_component_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::fs::symlink_dir;
    let temp = tempfile::tempdir()?;
    let real = temp.path().join("real");
    fs::create_dir(&real)?;
    let link = temp.path().join("link");
    match symlink_dir(&real, &link) {
        Ok(()) => assert!(SnapshotDirectory::open(&link).is_err()),
        Err(error) if error.raw_os_error() == Some(1314) => {
            eprintln!("symlink creation unavailable: privilege not held")
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[test]
fn native_rename_stays_in_the_held_parent() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let inner = dir.path().join("snapshots");
    fs::create_dir(&inner)?;
    let root = SnapshotDirectory::open(&inner).map_err(|e| format!("{e:?}"))?;
    let tmp = id(50)?;
    let target = id(51)?;
    let mut writer = root.create(&tmp)?;
    writer.write_all(b"native")?;
    writer.register(&target)?;
    assert!(!temp_path(&inner, &tmp).exists());
    assert_eq!(
        fs::read(inner.join(format!("{}.dusnap", target.as_str())))?,
        b"native"
    );
    assert!(
        !dir.path()
            .join(format!("{}.dusnap", target.as_str()))
            .exists()
    );
    Ok(())
}

#[test]
fn rename_collision_preserves_target_and_withdraws_temp() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let root = SnapshotDirectory::open(dir.path()).map_err(|e| format!("{e:?}"))?;
    let target = id(61)?;
    let target_path = dir.path().join(format!("{}.dusnap", target.as_str()));
    fs::write(&target_path, b"original")?;
    let tmp = id(62)?;
    let mut writer = root.create(&tmp)?;
    writer.write_all(b"replacement")?;
    assert!(fs::remove_file(temp_path(dir.path(), &tmp)).is_err());
    let error = writer
        .register(&target)
        .err()
        .ok_or("collision must fail")?;
    assert_eq!(error.raw_os_error(), Some(183));
    assert_eq!(fs::read(target_path)?, b"original");
    assert!(!temp_path(dir.path(), &tmp).exists());
    Ok(())
}

#[test]
fn competing_nt_renames_preserve_exactly_one_result() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = SnapshotDirectory::open(dir.path()).map_err(|e| format!("{e:?}"))?;
    let mut a = root.create(&id(70)?)?;
    let mut b = root.create(&id(71)?)?;
    a.write_all(b"one")?;
    b.write_all(b"two")?;
    let barrier = Arc::new(Barrier::new(2));
    let final_id = id(72)?;
    thread::scope(|scope| {
        let first_barrier = barrier.clone();
        let first_id = final_id.clone();
        let first = scope.spawn(move || {
            first_barrier.wait();
            a.register(&first_id)
        });
        let second = scope.spawn(move || {
            barrier.wait();
            b.register(&final_id)
        });
        assert_ne!(
            first.join().map_err(|_| "first panic")?.is_ok(),
            second.join().map_err(|_| "second panic")?.is_ok()
        );
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    let bytes = fs::read(dir.path().join(format!("{}.dusnap", id(72)?.as_str())))?;
    assert!(bytes == b"one" || bytes == b"two");
    Ok(())
}

#[test]
fn sealed_capture_reads_the_same_bytes_while_excluding_other_writers()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = SnapshotDirectory::open(dir.path()).map_err(|e| format!("{e:?}"))?;
    let temp = id(91)?;
    let path = temp_path(dir.path(), &temp);
    let mut capture = root.capture(&temp)?;
    capture.write_all(b"sealed")?;
    let mut sealed = capture.seal()?;
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    assert!(root.create(&temp).is_err());
    assert!(root.capture(&temp).is_err());
    let mut bytes = Vec::new();
    sealed.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"sealed");
    sealed.seek(SeekFrom::Start(2))?;
    let mut tail = [0; 4];
    sealed.read_exact(&mut tail)?;
    assert_eq!(&tail, b"aled");
    drop(sealed);
    assert_eq!(fs::read_dir(dir.path())?.count(), 0);
    Ok(())
}

#[test]
fn sealed_capture_is_independent_of_source_replacement_and_is_removed()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("source.bin");
    fs::write(&source, b"original")?;
    let snapshots = dir.path().join("snapshots");
    fs::create_dir(&snapshots)?;
    let root = SnapshotDirectory::open(&snapshots).map_err(|e| format!("{e:?}"))?;
    let temp = id(93)?;
    let path = temp_path(&snapshots, &temp);
    let mut capture = root.capture(&temp)?;
    std::io::copy(&mut fs::File::open(&source)?, &mut capture)?;
    let mut sealed = capture.seal()?;
    fs::remove_file(&source)?;
    fs::write(&source, b"replacement")?;
    assert!(fs::remove_file(&path).is_err());
    let mut actual = Vec::new();
    sealed.read_to_end(&mut actual)?;
    assert_eq!(actual, b"original");
    assert_eq!(fs::read(&source)?, b"replacement");
    drop(sealed);
    assert!(!path.exists());
    Ok(())
}

#[test]
fn abandoning_a_capture_removes_it_and_allows_name_reuse() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let root = SnapshotDirectory::open(dir.path()).map_err(|e| format!("{e:?}"))?;
    let temp = id(92)?;
    let mut capture = root.capture(&temp)?;
    capture.write_all(b"partial")?;
    drop(capture);
    assert_eq!(fs::read_dir(dir.path())?.count(), 0);
    drop(root.capture(&temp)?);
    assert_eq!(fs::read_dir(dir.path())?.count(), 0);
    Ok(())
}
