use super::invalid;
use du_core_contracts::ports::PortError;

pub(super) fn bytes(name: &[u16]) -> Vec<u8> {
    name.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub(super) fn wide(name: &[u8]) -> Result<Vec<u16>, PortError> {
    if name.is_empty() || !name.len().is_multiple_of(2) {
        return Err(invalid());
    }
    let result: Vec<_> = name
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .collect();
    if result
        .iter()
        .any(|v| *v == 0 || *v == b'\\' as u16 || *v == b'/' as u16 || *v == b':' as u16)
        || result == [46]
        || result == [46, 46]
    {
        return Err(invalid());
    }
    Ok(result)
}
