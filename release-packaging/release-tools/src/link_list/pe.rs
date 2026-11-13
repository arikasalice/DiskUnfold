//! Imported DLLs of a PE executable (import and delay-load directories).

fn u16_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes(
        bytes.get(at..at + 2)?.try_into().ok()?,
    )))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?)).ok()
}

struct Section {
    virtual_address: usize,
    span: usize,
    raw_offset: usize,
}

fn rva_to_offset(sections: &[Section], rva: usize) -> Option<usize> {
    sections.iter().find_map(|section| {
        let delta = rva.checked_sub(section.virtual_address)?;
        (delta < section.span).then(|| section.raw_offset + delta)
    })
}

fn c_string(bytes: &[u8], at: usize) -> Option<String> {
    let tail = bytes.get(at..)?;
    let end = tail.iter().position(|byte| *byte == 0)?;
    Some(String::from_utf8_lossy(&tail[..end]).into_owned())
}

/// Lists the DLL names the executable imports, including delay-loaded ones.
///
/// # Errors
/// Returns a message when the file is not a readable PE image.
pub fn imports(bytes: &[u8]) -> Result<Vec<String>, String> {
    let bad = || "malformed PE file".to_owned();
    let header = u32_at(bytes, 0x3c).ok_or_else(bad)?;
    if bytes.get(header..header + 4) != Some(&b"PE\0\0"[..]) {
        return Err(bad());
    }
    let coff = header + 4;
    let section_count = u16_at(bytes, coff + 2).ok_or_else(bad)?;
    let optional_size = u16_at(bytes, coff + 16).ok_or_else(bad)?;
    let optional = coff + 20;
    let directories = match u16_at(bytes, optional).ok_or_else(bad)? {
        0x10b => optional + 96,
        0x20b => optional + 112,
        _ => return Err(bad()),
    };
    let table = optional + optional_size;
    let sections: Vec<Section> = (0..section_count)
        .map(|index| {
            let at = table + index * 40;
            let virtual_size = u32_at(bytes, at + 8)?;
            let raw_size = u32_at(bytes, at + 16)?;
            Some(Section {
                virtual_address: u32_at(bytes, at + 12)?,
                span: virtual_size.max(raw_size),
                raw_offset: u32_at(bytes, at + 20)?,
            })
        })
        .collect::<Option<_>>()
        .ok_or_else(bad)?;
    let mut names = Vec::new();
    // (directory index, descriptor size, offset of the name field)
    for (index, size, name_field) in [(1_usize, 20_usize, 12_usize), (13, 32, 4)] {
        let rva = u32_at(bytes, directories + index * 8).ok_or_else(bad)?;
        if rva == 0 {
            continue;
        }
        let mut at = rva_to_offset(&sections, rva).ok_or_else(bad)?;
        loop {
            let name_rva = u32_at(bytes, at + name_field).ok_or_else(bad)?;
            if name_rva == 0 {
                break;
            }
            let name_at = rva_to_offset(&sections, name_rva).ok_or_else(bad)?;
            names.push(c_string(bytes, name_at).ok_or_else(bad)?);
            at += size;
        }
    }
    Ok(names)
}

#[cfg(test)]
pub(super) fn fixture(dlls: &[&str]) -> Vec<u8> {
    // DOS header, PE signature, COFF header, PE32+ optional header, one section that maps RVA
    // 0x1000 to the raw data holding the descriptors and names.
    let optional_size = 112 + 16 * 8;
    let raw_at = 0x200;
    let descriptors = (dlls.len() + 1) * 20;
    let mut names_at = descriptors;
    let mut data = vec![0_u8; descriptors];
    let mut names = Vec::new();
    for (index, dll) in dlls.iter().enumerate() {
        let rva = (0x1000 + names_at) as u32;
        data[index * 20 + 12..index * 20 + 16].copy_from_slice(&rva.to_le_bytes());
        names.extend(dll.bytes().chain(std::iter::once(0)));
        names_at += dll.len() + 1;
    }
    data.extend(names);
    let mut out = vec![0_u8; raw_at];
    out[0] = b'M';
    out[1] = b'Z';
    out[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
    out[0x80..0x84].copy_from_slice(b"PE\0\0");
    out[0x84 + 2..0x84 + 4].copy_from_slice(&1_u16.to_le_bytes());
    out[0x84 + 16..0x84 + 18].copy_from_slice(&(optional_size as u16).to_le_bytes());
    let optional = 0x84 + 20;
    out[optional..optional + 2].copy_from_slice(&0x20b_u16.to_le_bytes());
    out[optional + 112 + 8..optional + 112 + 12].copy_from_slice(&0x1000_u32.to_le_bytes());
    let section = optional + optional_size;
    out[section + 8..section + 12].copy_from_slice(&(data.len() as u32).to_le_bytes());
    out[section + 12..section + 16].copy_from_slice(&0x1000_u32.to_le_bytes());
    out[section + 16..section + 20].copy_from_slice(&(data.len() as u32).to_le_bytes());
    out[section + 20..section + 24].copy_from_slice(&(raw_at as u32).to_le_bytes());
    out.extend(data);
    out
}
