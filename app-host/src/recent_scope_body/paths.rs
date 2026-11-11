//! Validate stored native path syntax without interpreting comparison keys.
use super::{BodyError, Conditions, Exclusion, RecentTarget};
use du_core_contracts::{OsPath, PathEncoding};

pub(super) fn validate(scope: &Conditions, targets: &[RecentTarget]) -> Result<(), BodyError> {
    if targets.is_empty() || targets.len() != scope.includes.len() {
        return Err(BodyError::Malformed);
    }
    for (target, key) in targets.iter().zip(&scope.includes) {
        let encoding = &target.applied_path.encoding;
        if encoding != &key.encoding
            || encoding != &target.entry_path.encoding
            || encoding != &target.relative_path.encoding
            || !target.volume_id.bytes().is_ok_and(|raw| !raw.is_empty())
        {
            return Err(BodyError::Malformed);
        }
        validate_path(&target.applied_path, true)?;
        validate_path(&target.entry_path, true)?;
        validate_path(&target.relative_path, false)?;
    }
    for mount in &scope.included_mounts {
        validate_pair(&mount.path, &mount.key.encoding)?;
    }
    for exclusion in &scope.exclusions {
        match exclusion {
            Exclusion::Path { path, key } => validate_pair(path, &key.encoding)?,
            Exclusion::NamePattern { pattern } if pattern.is_empty() => {
                return Err(BodyError::Malformed);
            }
            Exclusion::NamePattern { .. } => {}
        }
    }
    Ok(())
}

fn validate_pair(path: &OsPath, encoding: &PathEncoding) -> Result<(), BodyError> {
    if &path.encoding != encoding {
        return Err(BodyError::Malformed);
    }
    validate_path(path, true)
}

fn validate_path(path: &OsPath, absolute: bool) -> Result<(), BodyError> {
    let raw = path.raw.bytes().map_err(|_| BodyError::Malformed)?;
    let valid = match path.encoding {
        PathEncoding::Unix => unix(&raw, absolute),
        PathEncoding::WindowsUtf16le => windows(&raw, absolute),
    };
    if valid {
        Ok(())
    } else {
        Err(BodyError::Malformed)
    }
}

fn unix(raw: &[u8], absolute: bool) -> bool {
    let rooted = raw.starts_with(b"/");
    (!absolute || rooted)
        && !raw.contains(&0)
        && (absolute || !raw.starts_with(b"//"))
        && raw
            .split(|byte| *byte == b'/')
            .all(|part| part != b"." && part != b"..")
}

const BACKSLASH: u16 = b'\\' as u16;
const COLON: u16 = b':' as u16;
const DOT: u16 = b'.' as u16;
const EXTENDED: &[u16] = &[BACKSLASH, BACKSLASH, b'?' as u16, BACKSLASH];
const UNC: &[u16] = &[b'U' as u16, b'N' as u16, b'C' as u16, BACKSLASH];
const VOLUME: &[u16] = &[
    b'V' as u16,
    b'o' as u16,
    b'l' as u16,
    b'u' as u16,
    b'm' as u16,
    b'e' as u16,
    b'{' as u16,
];

fn windows(raw: &[u8], absolute: bool) -> bool {
    let (pairs, tail) = raw.as_chunks::<2>();
    if !tail.is_empty() {
        return false;
    }
    let wide: Vec<_> = pairs.iter().copied().map(u16::from_le_bytes).collect();
    if wide.contains(&0) {
        return false;
    }
    if absolute {
        return windows_absolute(&wide).is_some_and(windows_components);
    }
    let relative = wide.strip_prefix(&[BACKSLASH]).unwrap_or(&wide);
    !relative.starts_with(&[BACKSLASH]) && windows_components(relative)
}

fn windows_absolute(wide: &[u16]) -> Option<&[u16]> {
    let (path, extended) = wide
        .strip_prefix(EXTENDED)
        .map_or((wide, false), |path| (path, true));
    if let [letter, COLON, BACKSLASH, rest @ ..] = path
        && u8::try_from(*letter).is_ok_and(|letter| letter.is_ascii_alphabetic())
    {
        return Some(rest);
    }
    let network = if extended {
        path.strip_prefix(UNC)
    } else {
        path.strip_prefix(&[BACKSLASH, BACKSLASH])
    };
    if let Some(network) = network {
        let mut parts = network.split(|unit| *unit == BACKSLASH);
        let (server, share) = (parts.next()?, parts.next()?);
        return (!server.is_empty() && !share.is_empty()).then_some(network);
    }
    if extended {
        let volume = path.strip_prefix(VOLUME)?;
        let (name, rest) = volume.split_at_checked(37)?;
        if name[36] == b'}' as u16 && valid_guid(&name[..36]) {
            return rest.strip_prefix(&[BACKSLASH]);
        }
    }
    None
}

fn valid_guid(name: &[u16]) -> bool {
    name.iter().enumerate().all(|(index, unit)| {
        if [8, 13, 18, 23].contains(&index) {
            *unit == b'-' as u16
        } else {
            u8::try_from(*unit).is_ok_and(|byte| byte.is_ascii_hexdigit())
        }
    })
}

fn windows_components(wide: &[u16]) -> bool {
    !wide.contains(&COLON)
        && !wide.contains(&(b'/' as u16))
        && wide
            .split(|unit| *unit == BACKSLASH)
            .all(|part| part != [DOT] && part != [DOT, DOT])
}
