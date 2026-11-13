//! Confirm the current mount entries of one saved Windows volume GUID.
use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    volume::{RecentVolumeEntry, RecentVolumeObservation, RecentVolumePort},
};
use windows::{
    Win32::{
        Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_INVALID_DRIVE, ERROR_NO_MORE_FILES, ERROR_PATH_NOT_FOUND,
            HANDLE,
        },
        Storage::FileSystem::{
            FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetVolumePathNamesForVolumeNameW,
        },
    },
    core::{Error, HRESULT, PCWSTR},
};

use super::{WindowsVolumes, bytes, drive::letter as drive_letter, identity, terminated};

const IDENTITY_KIND: &[u8] = b"windows_volume_guid";
const BACKSLASH: u16 = b'\\' as u16;
const EXTENDED: &[u16] = &[BACKSLASH, BACKSLASH, b'?' as u16, BACKSLASH];
// Match the native handle-path buffer bound. A larger mount list is unverified.
const MAX_PATH_UNITS: usize = 32_768;

impl RecentVolumePort for WindowsVolumes {
    fn recent_volume(
        &self,
        saved_id: &[u8],
        saved_entry: &OsPath,
        continue_work: &dyn Fn() -> bool,
    ) -> Option<RecentVolumeObservation> {
        let name = guid_name(saved_id)?;
        let entry = saved_drive(saved_entry)?;
        if !continue_work() {
            return None;
        }
        let entries = match mount_paths(&name) {
            Ok(Some(paths)) => verify_paths(&name, saved_id, entry, paths, continue_work)?,
            Err(error) if missing(&error) => {
                if !inventory_excludes(saved_id, continue_work)? || !continue_work() {
                    return None;
                }
                match mount_paths(&name) {
                    Err(error) if missing(&error) => Vec::new(),
                    _ => return None,
                }
            }
            _ => return None,
        };
        continue_work().then_some(RecentVolumeObservation {
            identity_kind: IDENTITY_KIND,
            entries,
        })
    }
}

fn wide(raw: &[u8]) -> Option<Vec<u16>> {
    let (pairs, tail) = raw.as_chunks::<2>();
    tail.is_empty()
        .then(|| pairs.iter().copied().map(u16::from_le_bytes).collect())
}

fn guid_name(raw: &[u8]) -> Option<Vec<u16>> {
    let units = wide(raw)?;
    let text = String::from_utf16(&units).ok()?;
    let guid = text.strip_prefix(r"\\?\Volume{")?.strip_suffix('}')?;
    if guid.len() != 36
        || !guid.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return None;
    }
    let mut units = units;
    units.push(BACKSLASH);
    Some(terminated(&units))
}

fn saved_drive(path: &OsPath) -> Option<u16> {
    if path.encoding != PathEncoding::WindowsUtf16le {
        return None;
    }
    let units = wide(&path.raw.bytes().ok()?)?;
    drive_letter(units.strip_prefix(EXTENDED).unwrap_or(&units))
}

fn mount_paths(name: &[u16]) -> Result<Option<Vec<Vec<u16>>>, Error> {
    let mut result = vec![0; MAX_PATH_UNITS];
    let mut copied = 0;
    // SAFETY: `name` is a validated, NUL-terminated volume GUID path. Both output
    // arguments point to live writable storage for the duration of the call.
    unsafe {
        GetVolumePathNamesForVolumeNameW(PCWSTR(name.as_ptr()), Some(&mut result), &mut copied)?
    };
    Ok(result.get(..copied as usize).and_then(parse_mounts))
}

fn parse_mounts(buffer: &[u16]) -> Option<Vec<Vec<u16>>> {
    let list = buffer.strip_suffix(&[0])?;
    if list.is_empty() || list == [0] {
        return Some(Vec::new());
    }
    let list = list.strip_suffix(&[0])?;
    let mut paths = list
        .split(|unit| *unit == 0)
        .map(<[u16]>::to_vec)
        .collect::<Vec<_>>();
    if paths
        .iter()
        .any(|path| path.is_empty() || !path.ends_with(&[BACKSLASH]))
    {
        return None;
    }
    paths.sort_unstable();
    paths.dedup();
    Some(paths)
}

fn verify_paths(
    name: &[u16],
    saved_id: &[u8],
    saved_entry: u16,
    paths: Vec<Vec<u16>>,
    continue_work: &dyn Fn() -> bool,
) -> Option<Vec<RecentVolumeEntry>> {
    // A present volume without an accessible entry is not confirmed absent.
    if paths.is_empty() {
        return None;
    }
    let entries = paths
        .iter()
        .map(|path| {
            drive_letter(path.get(..3)?)?;
            if !continue_work() || !identity(path).ok()?.eq_ignore_ascii_case(saved_id) {
                return None;
            }
            let same_entry = drive_letter(path) == Some(saved_entry);
            let native: Vec<_> = EXTENDED.iter().chain(path).copied().collect();
            let display = String::from_utf16(&native);
            Some(RecentVolumeEntry {
                path: OsPath {
                    encoding: PathEncoding::WindowsUtf16le,
                    raw: PathBytes::from_bytes(&bytes(&native)),
                    lossy: display.is_err(),
                    display: display.unwrap_or_else(|_| String::from_utf16_lossy(&native)),
                },
                same_entry,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if !continue_work() || mount_paths(name).ok()?? != paths {
        return None;
    }
    Some(entries)
}

fn missing(error: &Error) -> bool {
    [
        ERROR_FILE_NOT_FOUND,
        ERROR_PATH_NOT_FOUND,
        ERROR_INVALID_DRIVE,
    ]
    .iter()
    .any(|code| error.code() == HRESULT::from_win32(code.0))
}

struct VolumeSearch(HANDLE);

impl Drop for VolumeSearch {
    fn drop(&mut self) {
        // SAFETY: this guard owns a successful FindFirstVolumeW search handle.
        let _ = unsafe { FindVolumeClose(self.0) };
    }
}

fn inventory_excludes(saved_id: &[u8], continue_work: &dyn Fn() -> bool) -> Option<bool> {
    if !continue_work() {
        return None;
    }
    let mut name = [0u16; 64];
    // SAFETY: `name` points to a live writable buffer.
    let search = VolumeSearch(unsafe { FindFirstVolumeW(&mut name) }.ok()?);
    loop {
        let end = name.iter().position(|unit| *unit == 0)?;
        let observed = crate::windows_observation::volume_name(&name[..end]).ok()?;
        if observed.eq_ignore_ascii_case(saved_id) {
            return Some(false);
        }
        if !continue_work() {
            return None;
        }
        name.fill(0);
        // SAFETY: the guard keeps the search handle live and `name` is writable.
        match unsafe { FindNextVolumeW(search.0, &mut name) } {
            Ok(()) => {}
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => {
                return Some(true);
            }
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests;
