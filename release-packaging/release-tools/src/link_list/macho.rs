//! LC_LOAD_DYLIB style dependencies of a Mach-O executable (thin or universal).

fn u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u32_be(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

const MAGIC_64: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];
const FAT_MAGIC: [u8; 4] = [0xca, 0xfe, 0xba, 0xbe];
const LOAD_COMMANDS: [u32; 5] = [0x0c, 0x20, 0x8000_0018, 0x8000_001f, 0x8000_0023];

fn thin(slice: &[u8]) -> Option<Vec<String>> {
    if slice.get(..4)? != MAGIC_64 {
        return None;
    }
    let commands = usize::try_from(u32_le(slice, 16)?).ok()?;
    let mut at = 32_usize;
    let mut names = Vec::new();
    for _ in 0..commands {
        let command = u32_le(slice, at)?;
        let size = usize::try_from(u32_le(slice, at + 4)?).ok()?;
        if size < 8 {
            return None;
        }
        if LOAD_COMMANDS.contains(&command) {
            let name = at + usize::try_from(u32_le(slice, at + 8)?).ok()?;
            let tail = slice.get(name..at.checked_add(size)?)?;
            let end = tail.iter().position(|byte| *byte == 0)?;
            names.push(String::from_utf8_lossy(&tail[..end]).into_owned());
        }
        at = at.checked_add(size)?;
    }
    Some(names)
}

/// Lists the dylibs of every architecture slice.
///
/// # Errors
/// Returns a message when the file is not a readable 64-bit Mach-O or universal image.
pub fn dylibs(bytes: &[u8]) -> Result<Vec<String>, String> {
    let bad = || "malformed Mach-O file".to_owned();
    if bytes.get(..4) != Some(&FAT_MAGIC[..]) {
        return thin(bytes).ok_or_else(bad);
    }
    let count = usize::try_from(u32_be(bytes, 4).ok_or_else(bad)?).map_err(|_| bad())?;
    let mut all = Vec::new();
    for index in 0..count {
        let entry = 8 + index * 20;
        let offset =
            usize::try_from(u32_be(bytes, entry + 8).ok_or_else(bad)?).map_err(|_| bad())?;
        let size =
            usize::try_from(u32_be(bytes, entry + 12).ok_or_else(bad)?).map_err(|_| bad())?;
        let slice = bytes
            .get(offset..offset.checked_add(size).ok_or_else(bad)?)
            .ok_or_else(bad)?;
        all.extend(thin(slice).ok_or_else(bad)?);
    }
    Ok(all)
}

#[cfg(test)]
pub(super) fn fixture(libraries: &[&str]) -> Vec<u8> {
    let mut commands = Vec::new();
    for name in libraries {
        let size = (24 + name.len() + 1).div_ceil(8) * 8;
        let mut command = vec![0_u8; size];
        command[..4].copy_from_slice(&0x0c_u32.to_le_bytes());
        command[4..8].copy_from_slice(&(size as u32).to_le_bytes());
        command[8..12].copy_from_slice(&24_u32.to_le_bytes());
        command[24..24 + name.len()].copy_from_slice(name.as_bytes());
        commands.extend(command);
    }
    let mut out = vec![0_u8; 32];
    out[..4].copy_from_slice(&MAGIC_64);
    out[16..20].copy_from_slice(&(libraries.len() as u32).to_le_bytes());
    out[20..24].copy_from_slice(&(commands.len() as u32).to_le_bytes());
    out.extend(commands);
    out
}

#[cfg(test)]
pub(super) fn universal(slices: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(FAT_MAGIC);
    out.extend((slices.len() as u32).to_be_bytes());
    let mut offset = 8 + slices.len() * 20;
    for slice in slices {
        out.extend([0_u8; 8]);
        out.extend((offset as u32).to_be_bytes());
        out.extend((slice.len() as u32).to_be_bytes());
        out.extend([0_u8; 4]);
        offset += slice.len();
    }
    for slice in slices {
        out.extend(slice);
    }
    out
}
