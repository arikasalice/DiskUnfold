use crate::macos_observation::MacScopeRoots;
use du_core_contracts::scan_observation::ScopeRootPort;
use du_core_contracts::volume::{ConfirmedFirmlink, RootVolumeGroup, RootVolumeGroupObservation};
use rustix::{
    fs::{AtFlags, CWD, FileType, statat},
    io::Errno,
};
use std::{ffi::CString, fs};

pub(super) const MNT_EXT_ROOT_DATA_VOL: u32 = 0x0000_0001;

#[derive(Clone)]
struct DataMount {
    path: Vec<u8>,
    device: u64,
    inode: u64,
}

type FirmlinkRow = (Vec<u8>, Vec<u8>);

pub(super) fn observe(entries: &[libc::statfs]) -> RootVolumeGroupObservation {
    let Ok(Some(root)) = directory_identity(b"/") else {
        return RootVolumeGroupObservation::Unconfirmed;
    };
    let flagged: Vec<_> = entries
        .iter()
        .filter(|entry| entry.f_flags_ext & MNT_EXT_ROOT_DATA_VOL != 0)
        .collect();
    if flagged.len() != 1 {
        return RootVolumeGroupObservation::Unconfirmed;
    }
    let Some(path) = field(&flagged[0].f_mntonname) else {
        return RootVolumeGroupObservation::Unconfirmed;
    };
    let Ok(Some((device, _))) = directory_identity(&path) else {
        return RootVolumeGroupObservation::Unconfirmed;
    };
    let Some(inode) = enumerated_mount_inode(&path, root.0) else {
        return RootVolumeGroupObservation::Unconfirmed;
    };
    let data_mounts = [DataMount {
        path,
        device,
        inode,
    }];
    let Ok(contents) = fs::read("/usr/share/firmlinks") else {
        return RootVolumeGroupObservation::Unconfirmed;
    };
    let Ok(rows) = parse_firmlinks(&contents) else {
        return RootVolumeGroupObservation::Unconfirmed;
    };
    let data = &data_mounts[0];
    let mut firmlinks = Vec::with_capacity(rows.len());
    for (source, relative) in rows {
        let target = [data.path.as_slice(), b"/", relative.as_slice()].concat();
        let source_identity = directory_identity(&source);
        let target_identity = directory_identity(&target);
        match matching_firmlink(source, target, source_identity, target_identity) {
            Ok(Some(firmlink)) => firmlinks.push(firmlink),
            Ok(None) => {}
            Err(()) => return RootVolumeGroupObservation::Unconfirmed,
        }
    }
    evaluate(root.0, &data_mounts, firmlinks)
        .map(RootVolumeGroupObservation::Confirmed)
        .unwrap_or(RootVolumeGroupObservation::Unconfirmed)
}

fn field(raw: &[libc::c_char]) -> Option<Vec<u8>> {
    let end = raw.iter().position(|&byte| byte == 0)?;
    Some(
        raw.iter()
            .take(end)
            .map(|&byte| u8::from_ne_bytes(byte.to_ne_bytes()))
            .collect(),
    )
}

fn enumerated_mount_inode(data_mount: &[u8], group_device: u64) -> Option<u64> {
    let separator = data_mount.iter().rposition(|&byte| byte == b'/')?;
    let parent_path = if separator == 0 {
        b"/".as_slice()
    } else {
        &data_mount[..separator]
    };
    let name = data_mount.get(separator + 1..)?;
    if name.is_empty() {
        return None;
    }
    let mut parent = MacScopeRoots.open_root(parent_path).ok()?;
    loop {
        let batch = parent.directory.next_batch().ok()?;
        if batch.is_empty() {
            return None;
        }
        if let Some(entry) = batch.iter().find(|entry| entry.name == name) {
            let metadata = entry.metadata.as_ref().ok()?;
            return (metadata.kind
                == du_core_contracts::scan_observation::ObservedKind::MountPoint
                && metadata.device == group_device)
                .then_some(metadata.inode);
        }
    }
}

fn directory_identity(path: &[u8]) -> Result<Option<(u64, u64)>, ()> {
    let path = CString::new(path).map_err(|_| ())?;
    let stat = match statat(CWD, path, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::Directory => stat,
        Ok(_) => return Err(()),
        Err(Errno::NOENT) => return Ok(None),
        Err(_) => return Err(()),
    };
    Ok(Some((u64::from(stat.st_dev.cast_unsigned()), stat.st_ino)))
}

fn matching_firmlink(
    source: Vec<u8>,
    target: Vec<u8>,
    source_identity: Result<Option<(u64, u64)>, ()>,
    target_identity: Result<Option<(u64, u64)>, ()>,
) -> Result<Option<ConfirmedFirmlink>, ()> {
    match (source_identity?, target_identity?) {
        (None, None) => Ok(None),
        (Some((device, inode)), Some((target_device, target_inode)))
            if (device, inode) == (target_device, target_inode) =>
        {
            Ok(Some(ConfirmedFirmlink {
                source,
                target,
                device,
                inode,
            }))
        }
        _ => Err(()),
    }
}

fn parse_firmlinks(contents: &[u8]) -> Result<Vec<FirmlinkRow>, ()> {
    let mut rows = Vec::new();
    for line in contents.split(|&byte| byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let Some(separator) = line.iter().position(|&byte| byte == b'\t') else {
            return Err(());
        };
        let (source, suffix) = line.split_at(separator);
        let relative = suffix.get(1..).ok_or(())?;
        if source.len() < 2
            || !source.starts_with(b"/")
            || source[1..].split(|&byte| byte == b'/').any(|part| {
                part.is_empty() || part == b"." || part == b".." || part.contains(&b'\t')
            })
            || relative.is_empty()
            || relative.starts_with(b"/")
            || relative.split(|&byte| byte == b'/').any(|part| {
                part.is_empty() || part == b"." || part == b".." || part.contains(&b'\t')
            })
        {
            return Err(());
        }
        rows.push((source.to_vec(), relative.to_vec()));
    }
    if rows.is_empty() {
        return Err(());
    }
    Ok(rows)
}

fn evaluate(
    root_device: u64,
    data_mounts: &[DataMount],
    mut firmlinks: Vec<ConfirmedFirmlink>,
) -> Option<RootVolumeGroup> {
    if data_mounts.len() != 1 || data_mounts[0].device != root_device || firmlinks.is_empty() {
        return None;
    }
    firmlinks.sort_by(|left, right| left.source.cmp(&right.source));
    let data = &data_mounts[0];
    Some(RootVolumeGroup {
        data_mount: data.path.clone(),
        device: root_device,
        data_inode: data.inode,
        firmlinks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(path: &[u8], device: u64, inode: u64) -> DataMount {
        DataMount {
            path: path.to_vec(),
            device,
            inode,
        }
    }

    fn row(source: &[u8], target: &[u8], device: u64, inode: u64) -> ConfirmedFirmlink {
        ConfirmedFirmlink {
            source: source.to_vec(),
            target: target.to_vec(),
            device,
            inode,
        }
    }

    #[test]
    fn parses_firmlink_rows_and_ignores_empty_lines() {
        assert_eq!(
            parse_firmlinks(b"/Users\tUsers\n\n/Applications\tApplications\r\n"),
            Ok(vec![
                (b"/Users".to_vec(), b"Users".to_vec()),
                (b"/Applications".to_vec(), b"Applications".to_vec()),
            ])
        );
    }

    #[test]
    fn rejects_malformed_firmlink_rows() {
        assert!(parse_firmlinks(b"/Users Users\n").is_err());
        assert!(parse_firmlinks(b"Users\tUsers\n").is_err());
        assert!(parse_firmlinks(b"/Users\t/Users\n").is_err());
        assert!(parse_firmlinks(b"/Users\t../Users\n").is_err());
    }

    #[test]
    fn confirms_only_one_matching_data_mount_and_equal_firmlink_identities() {
        let rows = || vec![row(b"/Users", b"/Data/Users", 7, 11)];
        assert!(evaluate(7, &[], rows()).is_none());
        assert!(evaluate(7, &[data(b"/Data", 7, 3), data(b"/Data2", 7, 4)], rows()).is_none());
        assert!(evaluate(7, &[data(b"/Data", 8, 3)], rows()).is_none());
        let confirmed = evaluate(7, &[data(b"/Data", 7, 3)], rows());
        assert_eq!(confirmed.as_ref().map(|group| group.data_inode), Some(3));
        assert_eq!(
            confirmed.map(|group| group.firmlinks[0].source.clone()),
            Some(b"/Users".to_vec())
        );
    }

    #[test]
    fn skips_only_firmlinks_missing_on_both_sides() {
        assert_eq!(
            matching_firmlink(
                b"/Internal".to_vec(),
                b"/Data/Internal".to_vec(),
                Ok(None),
                Ok(None)
            ),
            Ok(None)
        );
        assert!(evaluate(7, &[data(b"/Data", 7, 3)], Vec::new()).is_none());
    }

    #[test]
    fn rejects_one_sided_missing_and_non_not_found_errors() {
        let present = Ok(Some((7, 11)));
        let missing = Ok(None);
        let denied = Err(());
        assert!(matching_firmlink(b"/A".to_vec(), b"/D/A".to_vec(), missing, present).is_err());
        assert!(matching_firmlink(b"/A".to_vec(), b"/D/A".to_vec(), present, missing).is_err());
        assert!(matching_firmlink(b"/A".to_vec(), b"/D/A".to_vec(), denied, Ok(None)).is_err());
        assert!(matching_firmlink(b"/A".to_vec(), b"/D/A".to_vec(), Ok(None), denied).is_err());
        assert!(
            matching_firmlink(
                b"/A".to_vec(),
                b"/D/A".to_vec(),
                Ok(Some((7, 11))),
                Ok(Some((7, 12))),
            )
            .is_err()
        );
    }
}
