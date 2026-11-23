use super::*;
use du_core_contracts::{Id, snapshot_file::SnapshotFileSystem};
use std::{fs, io::Write};
#[cfg(windows)]
use std::io::Read;

fn id(value: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}"))
}

fn temp_path(directory: &Path, value: &Id) -> PathBuf {
    directory.join(format!(".tmp-{}.dusnap.part", value.as_str()))
}

#[test]
fn native_publish_collision_and_drop_behave_atomically() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let data = temp.path().join("data");
    fs::create_dir(&data)?;
    let filesystem = NativeSnapshotFileSystem::for_data_directory(&data);
    let snapshots = data.join("snapshots");
    let directory = filesystem.open_or_create_snapshots()?;
    directory.check_no_replace_register()?;
    assert!(fs::metadata(&snapshots)?.is_dir());
    assert_eq!(fs::read_dir(&snapshots)?.count(), 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&snapshots)?.permissions().mode() & 0o777,
            0o700
        );
    }

    let temporary_id = id(1)?;
    let final_id = id(2)?;
    let mut staged = directory.create_exclusive_temp(&temporary_id)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(temp_path(&snapshots, &temporary_id))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    staged.write_all(b"published")?;
    staged.sync_file()?;
    staged.register(&final_id)?;
    directory.sync_folder()?;
    drop(staged);
    assert!(!temp_path(&snapshots, &temporary_id).exists());
    assert_eq!(
        fs::read(snapshots.join(format!("{}.dusnap", final_id.as_str())))?,
        b"published"
    );

    let collision = id(3)?;
    let original = snapshots.join(format!("{}.dusnap", collision.as_str()));
    fs::write(&original, b"original")?;
    let collision_temp = id(4)?;
    let mut staged = directory.create_exclusive_temp(&collision_temp)?;
    staged.write_all(b"replacement")?;
    staged.sync_file()?;
    assert_eq!(
        staged.register(&collision).err().map(|error| error.kind()),
        Some(std::io::ErrorKind::AlreadyExists)
    );
    drop(staged);
    assert_eq!(fs::read(original)?, b"original");
    assert!(!temp_path(&snapshots, &collision_temp).exists());

    let abandoned = id(5)?;
    let mut staged = directory.create_exclusive_temp(&abandoned)?;
    staged.write_all(b"partial")?;
    assert!(temp_path(&snapshots, &abandoned).exists());
    drop(staged);
    assert!(!temp_path(&snapshots, &abandoned).exists());
    Ok(())
}

#[cfg(windows)]
#[test]
fn native_registered_read_is_handle_bound_and_capture_is_removed_on_drop()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let data = temp.path().join("data");
    fs::create_dir(&data)?;
    let filesystem = NativeSnapshotFileSystem::for_data_directory(&data);
    let snapshots = data.join("snapshots");
    let directory = filesystem.open_or_create_snapshots()?;
    let registered_id = id(20)?;
    let registered = snapshots.join(format!("{}.dusnap", registered_id.as_str()));
    let original = b"registered source contents";
    fs::write(&registered, original)?;

    let mut source = directory.open_final(&registered_id)?;
    let expected = u64::try_from(original.len())?;
    let mut capture = directory.create_capture(Some(expected))?;
    let mut first = [0; 5];
    source.read_exact(&mut first)?;
    capture.write_all(&first)?;

    fs::remove_file(&registered)?;
    fs::write(&registered, b"replacement")?;
    std::io::copy(&mut source, &mut capture)?;
    let mut sealed = capture.seal()?;
    let mut copied = Vec::new();
    sealed.read_to_end(&mut copied)?;
    assert_eq!(copied, original);
    drop(sealed);
    assert_eq!(fs::read(&registered)?, b"replacement");
    assert_eq!(fs::read_dir(&snapshots)?.count(), 1);

    let pending_id = id(21)?;
    fs::write(
        snapshots.join(format!(".tmp-{}.dusnap.part", pending_id.as_str())),
        b"not registered",
    )?;
    assert_eq!(
        directory
            .open_final(&pending_id)
            .err()
            .map(|error| error.kind()),
        Some(std::io::ErrorKind::NotFound)
    );
    Ok(())
}
