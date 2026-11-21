use du_core_contracts::volume::{ConfirmedFirmlink, RootVolumeGroup, RootVolumeGroupObservation};
use rustix::fs::{AtFlags, CWD, FileType, statat};
use std::{ffi::CString, fs};

pub(super) const MNT_EXT_ROOT_DATA_VOL: u32 = 0x0000_0001;

#[derive(Clone)]
struct DataMount {
    path: Vec<u8>,
    device: u64,
    inode: u64,
}

struct FirmlinkIdentity {
    source: Vec<u8>,
    target: Vec<u8>,
    source_identity: Option<(u64, u64)>,
    target_identity: Option<(u64, u64)>,
}

type FirmlinkRow = (Vec<u8>, Vec<u8>);

pub(super) fn observe(entries: &[libc::statfs]) -> RootVolumeGroupObservation {
    let Some(root) = directory_identity(b"/") else {
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
    let Some((device, inode)) = directory_identity(&path) else {
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
    let identities = rows
        .into_iter()
        .map(|(source, relative)| {
            let target = [data.path.as_slice(), b"/", relative.as_slice()].concat();
            FirmlinkIdentity {
                source_identity: directory_identity(&source),
                target_identity: directory_identity(&target),
                source,
                target,
            }
        })
        .collect::<Vec<_>>();
    evaluate(root.0, &data_mounts, identities)
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

fn directory_identity(path: &[u8]) -> Option<(u64, u64)> {
    let path = CString::new(path).ok()?;
    let stat = statat(CWD, path, AtFlags::SYMLINK_NOFOLLOW).ok()?;
    (FileType::from_raw_mode(stat.st_mode) == FileType::Directory)
        .then_some((u64::from(stat.st_dev.cast_unsigned()), stat.st_ino))
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
    mut rows: Vec<FirmlinkIdentity>,
) -> Option<RootVolumeGroup> {
    if data_mounts.len() != 1 || data_mounts[0].device != root_device || rows.is_empty() {
        return None;
    }
    let mut firmlinks = Vec::with_capacity(rows.len());
    for row in rows.drain(..) {
        let (source_device, source_inode) = row.source_identity?;
        let (target_device, target_inode) = row.target_identity?;
        if (source_device, source_inode) != (target_device, target_inode) {
            return None;
        }
        firmlinks.push(ConfirmedFirmlink {
            source: row.source,
            target: row.target,
            device: source_device,
            inode: source_inode,
        });
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

    fn row(source: &[u8], target: &[u8], left: (u64, u64), right: (u64, u64)) -> FirmlinkIdentity {
        FirmlinkIdentity {
            source: source.to_vec(),
            target: target.to_vec(),
            source_identity: Some(left),
            target_identity: Some(right),
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
        let rows = || vec![row(b"/Users", b"/Data/Users", (7, 11), (7, 11))];
        assert!(evaluate(7, &[], rows()).is_none());
        assert!(evaluate(7, &[data(b"/Data", 7, 3), data(b"/Data2", 7, 4)], rows()).is_none());
        assert!(evaluate(7, &[data(b"/Data", 8, 3)], rows()).is_none());
        assert!(
            evaluate(
                7,
                &[data(b"/Data", 7, 3)],
                vec![row(b"/Users", b"/Data/Users", (7, 11), (7, 12))],
            )
            .is_none()
        );
        let confirmed = evaluate(7, &[data(b"/Data", 7, 3)], rows());
        assert_eq!(confirmed.as_ref().map(|group| group.data_inode), Some(3));
        assert_eq!(
            confirmed.map(|group| group.firmlinks[0].source.clone()),
            Some(b"/Users".to_vec())
        );
    }
}
