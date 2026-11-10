// Experiments only: these helpers do not implement or advertise desktop.reveal.
mod name_hold_experiments {
    use crate::windows_observation::{self as obs, WindowsDirectory};
    use du_core_contracts::scan_observation::ObservedKind;
    use std::{
        fs::{self, File, OpenOptions},
        mem::size_of,
        os::windows::fs::OpenOptionsExt,
        path::Path,
    };
    use windows::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };

    type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Identity {
        guid: Vec<u8>,
        serial: u64,
        id: [u8; 16],
    }

    fn id(parent: &WindowsDirectory, file: &File) -> TestResult<Identity> {
        let mut value = FILE_ID_INFO::default();
        // SAFETY: file owns a live handle and value has the required layout and size.
        unsafe {
            GetFileInformationByHandleEx(
                obs::raw(file),
                FileIdInfo,
                std::ptr::addr_of_mut!(value).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )?;
        }
        if value.FileId.Identifier.iter().all(|byte| *byte == 0) {
            return Err("128-bit ID unavailable".into());
        }
        Ok(Identity {
            guid: parent.volume_guid.to_vec(),
            serial: value.VolumeSerialNumber,
            id: value.FileId.Identifier,
        })
    }
    fn open(
        parent: &WindowsDirectory,
        name: &str,
        directory: bool,
        stable: bool,
    ) -> TestResult<File> {
        let wide: Vec<u16> = name.encode_utf16().collect();
        obs::relative(parent.storage_handle(), &wide, directory, stable)
            .map_err(|e| format!("{e:?}").into())
    }
    fn checked(
        parent: &WindowsDirectory,
        name: &str,
        directory: bool,
        expected: &Identity,
    ) -> TestResult<File> {
        let file = open(parent, name, directory, true)?;
        let info = obs::info(&file).map_err(|e| format!("{e:?}"))?;
        let required = if directory {
            ObservedKind::Directory
        } else {
            ObservedKind::File
        };
        if obs::kind(info.dwFileAttributes) != required || &id(parent, &file)? != expected {
            return Err("identity or kind mismatch".into());
        }
        Ok(file)
    }
    fn delete_handle(path: &Path, directory: bool) -> std::io::Result<File> {
        let mut options = OpenOptions::new();
        options.access_mode(DELETE.0).share_mode(7);
        if directory {
            options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0);
        }
        options.open(path)
    }
    fn parent(path: &Path) -> TestResult<WindowsDirectory> {
        obs::open_verified_stable(path).map_err(|e| format!("{e:?}").into())
    }
    fn moved(path: &Path) -> std::path::PathBuf {
        path.with_file_name("moved")
    }

    #[test]
    fn held_identity_survives_or_prevents_name_replacement() -> TestResult<()> {
        for directory in [false, true] {
            let fixture = tempfile::tempdir()?;
            let ancestor = fixture.path().join("ancestor");
            let path = ancestor.join("target");
            fs::create_dir(&ancestor)?;
            if directory {
                fs::create_dir(&path)?;
            } else {
                fs::write(&path, b"original")?;
            }
            let owner = parent(&ancestor)?;
            let original = open(&owner, "target", directory, false)?;
            let expected = id(&owner, &original)?;
            assert_eq!(expected.serial, id(&owner, owner.storage_handle())?.serial);
            drop(original);
            let held = checked(&owner, "target", directory, &expected)?;
            assert_eq!(id(&owner, &held)?, expected);
            assert!(fs::rename(&ancestor, moved(&ancestor)).is_err());
            if directory {
                assert!(fs::rename(&path, moved(&path)).is_err());
                assert!(fs::remove_dir(&path).is_err());
                drop(held);
                fs::rename(&path, moved(&path))?;
                fs::create_dir(&path)?;
            } else {
                match fs::rename(&path, moved(&path)) {
                    Ok(()) => {
                        eprintln!("Observed: file rename allowed while metadata handle was held");
                        fs::write(&path, b"replacement")?;
                        assert_eq!(id(&owner, &held)?, expected);
                        assert!(checked(&owner, "target", false, &expected).is_err());
                        let replacement = open(&owner, "target", false, true)?;
                        assert_ne!(id(&owner, &replacement)?.id, expected.id);
                        continue;
                    }
                    Err(error) => {
                        eprintln!(
                            "Observed: file rename blocked while metadata handle was held: {error}"
                        );
                        assert!(matches!(error.raw_os_error(), Some(5 | 32 | 33)));
                        assert!(path.exists());
                        assert_eq!(id(&owner, &held)?, expected);
                        drop(held);
                        continue;
                    }
                }
            }
            assert!(checked(&owner, "target", directory, &expected).is_err());
            let replacement = open(&owner, "target", directory, true)?;
            assert_ne!(id(&owner, &replacement)?.id, expected.id);
        }
        Ok(())
    }

    #[test]
    fn preexisting_delete_access_blocks_stable_directory_guards() -> TestResult<()> {
        let fixture = tempfile::tempdir()?;
        let ancestor = fixture.path().join("ancestor");
        let target = ancestor.join("target");
        fs::create_dir(&ancestor)?;
        fs::create_dir(&target)?;
        let owner = parent(&ancestor)?;
        let preexisting = delete_handle(&target, true)?;
        assert!(open(&owner, "target", true, true).is_err());
        drop(preexisting);
        assert!(open(&owner, "target", true, true).is_ok());
        drop(owner);
        let preexisting_ancestor = delete_handle(&ancestor, true)?;
        assert!(parent(&ancestor).is_err());
        drop(preexisting_ancestor);
        assert!(parent(&ancestor).is_ok());
        Ok(())
    }
}
