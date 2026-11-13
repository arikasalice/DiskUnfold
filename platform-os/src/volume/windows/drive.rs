//! Interpret a native Windows logical drive root.

/// Returns the canonical drive letter of a complete drive-root path.
pub(super) fn letter(units: &[u16]) -> Option<u16> {
    const COLON: u16 = b':' as u16;
    const SEPARATOR: u16 = b'\\' as u16;
    let [letter, COLON, SEPARATOR] = units else {
        return None;
    };
    let letter = u8::try_from(*letter).ok()?;
    letter
        .is_ascii_alphabetic()
        .then(|| u16::from(letter.to_ascii_uppercase()))
}
