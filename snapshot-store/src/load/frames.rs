use super::LoadError;
use std::io::Read;

pub(super) fn read_record<R: Read>(
    reader: &mut R,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<(u8, Vec<u8>)>, LoadError> {
    if cancelled() {
        return Err(LoadError::Cancelled);
    }
    let mut length = [0_u8; 4];
    if reader.read(&mut length[..1])? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut length[1..])?;
    let length = u32::from_le_bytes(length) as usize;
    if !(1..=65_536).contains(&length) {
        return Err(LoadError::InvalidSnapshot);
    }
    let mut record = vec![0; length];
    reader.read_exact(&mut record)?;
    Ok(Some((record[0], record[1..].to_vec())))
}
