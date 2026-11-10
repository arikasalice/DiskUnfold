//! Open a verified directory while retaining its ancestor handles.
use super::*;

pub(super) fn open_verified_impl(path: &Path, stable: bool) -> Result<WindowsDirectory, PortError> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(invalid());
    };
    let (Prefix::Disk(drive) | Prefix::VerbatimDisk(drive)) = prefix.kind() else {
        return Err(invalid());
    };
    if components.next() != Some(Component::RootDir) {
        return Err(invalid());
    }
    let root = [u16::from(drive), b':' as u16, b'\\' as u16, 0];
    // SAFETY: root is NUL-terminated and remains valid throughout these calls.
    if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } != 3 {
        return Err(invalid());
    }
    let handle = unsafe {
        CreateFileW(
            PCWSTR(root.as_ptr()),
            (FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE).0,
            if stable {
                FILE_SHARE_READ | FILE_SHARE_WRITE
            } else {
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
            },
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL,
            None,
        )
    }
    .map_err(win_error)?;
    // SAFETY: CreateFileW succeeded and transferred ownership of this handle.
    let mut file = unsafe { File::from_raw_handle(handle.0) };
    let root_info = info(&file)?;
    recall::check_handle_attributes(root_info.dwFileAttributes)?;
    if kind(root_info.dwFileAttributes) != ObservedKind::Directory || identity(&root_info) == 0 {
        return Err(invalid());
    }
    let canonical = final_path(&file, FILE_NAME_NORMALIZED)?;
    let expected = [
        92u16,
        92,
        63,
        92,
        u16::from(drive.to_ascii_uppercase()),
        58,
        92,
    ];
    if canonical.len() != expected.len()
        || !canonical
            .iter()
            .zip(expected)
            .all(|(a, b)| (*a == b) || (*a >= 97 && *a <= 122 && *a - 32 == b))
    {
        return Err(invalid());
    }
    let volume = u64::from(root_info.dwVolumeSerialNumber);
    let volume_guid = scope::volume_name(&final_path(&file, VOLUME_NAME_GUID)?)?.into();
    let mut filesystem = [0u16; 32];
    // SAFETY: the writable name buffer and open handle are valid for the call.
    unsafe {
        GetVolumeInformationByHandleW(raw(&file), None, None, None, None, Some(&mut filesystem))
    }
    .map_err(win_error)?;
    if !filesystem.starts_with(&[78, 84, 70, 83, 0]) {
        return Err(invalid());
    }
    let mut ancestors = AncestorChain {
        _handles: Vec::new(),
        ids: Vec::new(),
    };
    let mut parent_id = identity(&root_info);
    for component in components {
        let Component::Normal(name) = component else {
            return Err(invalid());
        };
        let name: Vec<_> = OsStr::new(name).encode_wide().collect();
        wide(&bytes(&name))?;
        let observed = recall::lookup(&file, &name)?;
        let next = recall::observe_if_safe(observed.attributes, || {
            verified_directory(&file, &name, volume, Some(observed.inode), stable)
        })?;
        ancestors.ids.push(parent_id.to_le_bytes().to_vec());
        parent_id = observed.inode;
        ancestors._handles.push(file);
        file = next;
    }
    Ok(WindowsDirectory {
        handle: file,
        volume,
        volume_guid,
        restart: true,
        pending: VecDeque::new(),
        finished: false,
        ancestors: Some(ancestors),
    })
}
