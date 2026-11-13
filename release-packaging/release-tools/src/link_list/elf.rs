//! DT_NEEDED entries of a 64-bit little-endian ELF executable.

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

fn usize_at(bytes: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u64_at(bytes, at)?).ok()
}

fn c_string(bytes: &[u8], at: usize) -> Option<String> {
    let tail = bytes.get(at..)?;
    let end = tail.iter().position(|byte| *byte == 0)?;
    Some(String::from_utf8_lossy(&tail[..end]).into_owned())
}

struct Segment {
    kind: u32,
    offset: usize,
    vaddr: usize,
    file_size: usize,
}

fn segments(bytes: &[u8]) -> Option<Vec<Segment>> {
    let table = usize_at(bytes, 0x20)?;
    let entry = usize::from(u16_at(bytes, 0x36)?);
    let count = usize::from(u16_at(bytes, 0x38)?);
    (0..count)
        .map(|index| {
            let at = table.checked_add(index.checked_mul(entry)?)?;
            Some(Segment {
                kind: u32_at(bytes, at)?,
                offset: usize_at(bytes, at + 8)?,
                vaddr: usize_at(bytes, at + 16)?,
                file_size: usize_at(bytes, at + 32)?,
            })
        })
        .collect()
}

fn file_offset(loads: &[&Segment], vaddr: usize) -> Option<usize> {
    loads.iter().find_map(|segment| {
        let delta = vaddr.checked_sub(segment.vaddr)?;
        (delta < segment.file_size).then(|| segment.offset + delta)
    })
}

/// Lists the shared libraries the executable asks the loader for.
///
/// # Errors
/// Returns a message when the file is not a readable 64-bit little-endian ELF image. A statically
/// linked executable has no dynamic section and returns an empty list.
pub fn needed(bytes: &[u8]) -> Result<Vec<String>, String> {
    let bad = || "malformed ELF file".to_owned();
    if bytes.get(4) != Some(&2) || bytes.get(5) != Some(&1) {
        return Err("only 64-bit little-endian ELF files are supported".to_owned());
    }
    let segments = segments(bytes).ok_or_else(bad)?;
    let loads: Vec<&Segment> = segments
        .iter()
        .filter(|segment| segment.kind == 1)
        .collect();
    let Some(dynamic) = segments.iter().find(|segment| segment.kind == 2) else {
        return Ok(Vec::new());
    };
    let mut needed = Vec::new();
    let mut strtab = None;
    let mut at = dynamic.offset;
    let end = dynamic
        .offset
        .checked_add(dynamic.file_size)
        .ok_or_else(bad)?;
    while at + 16 <= end {
        let tag = u64_at(bytes, at).ok_or_else(bad)?;
        let value = usize_at(bytes, at + 8).ok_or_else(bad)?;
        match tag {
            0 => break,
            1 => needed.push(value),
            5 => strtab = Some(value),
            _ => {}
        }
        at += 16;
    }
    let strtab = file_offset(&loads, strtab.ok_or_else(bad)?).ok_or_else(bad)?;
    needed
        .into_iter()
        .map(|name| c_string(bytes, strtab + name).ok_or_else(bad))
        .collect()
}

#[cfg(test)]
pub(super) fn fixture(libraries: &[&str]) -> Vec<u8> {
    // One PT_LOAD and one PT_DYNAMIC over a tiny file: header, two program headers, dynamic
    // section, string table.
    let strings: Vec<u8> = std::iter::once(0)
        .chain(
            libraries
                .iter()
                .flat_map(|name| name.bytes().chain(std::iter::once(0))),
        )
        .collect();
    let dynamic_at = 64 + 2 * 56;
    let dynamic_len = (libraries.len() + 2) * 16;
    let strtab_at = dynamic_at + dynamic_len;
    let total = strtab_at + strings.len();
    let mut out = vec![0_u8; total];
    out[..4].copy_from_slice(b"\x7fELF");
    out[4] = 2;
    out[5] = 1;
    out[0x20..0x28].copy_from_slice(&64_u64.to_le_bytes());
    out[0x36..0x38].copy_from_slice(&56_u16.to_le_bytes());
    out[0x38..0x3a].copy_from_slice(&2_u16.to_le_bytes());
    let mut put = |at: usize, kind: u32, offset: u64, vaddr: u64, size: u64| {
        out[at..at + 4].copy_from_slice(&kind.to_le_bytes());
        out[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        out[at + 16..at + 24].copy_from_slice(&vaddr.to_le_bytes());
        out[at + 32..at + 40].copy_from_slice(&size.to_le_bytes());
    };
    // Virtual addresses sit 0x1000 above file offsets to exercise the translation.
    put(64, 1, 0, 0x1000, total as u64);
    put(
        64 + 56,
        2,
        dynamic_at as u64,
        (0x1000 + dynamic_at) as u64,
        dynamic_len as u64,
    );
    let mut entries: Vec<(u64, u64)> = Vec::new();
    let mut offset = 1;
    for name in libraries {
        entries.push((1, offset));
        offset += name.len() as u64 + 1;
    }
    entries.push((5, (0x1000 + strtab_at) as u64));
    entries.push((0, 0));
    for (index, (tag, value)) in entries.iter().enumerate() {
        let at = dynamic_at + index * 16;
        out[at..at + 8].copy_from_slice(&tag.to_le_bytes());
        out[at + 8..at + 16].copy_from_slice(&value.to_le_bytes());
    }
    out[strtab_at..].copy_from_slice(&strings);
    out
}
