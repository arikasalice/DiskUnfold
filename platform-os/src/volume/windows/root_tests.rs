use super::*;

#[test]
fn opened_root_rejects_a_directory_on_the_same_volume() -> Result<(), Box<dyn std::error::Error>> {
    let volume: Vec<u16> = r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}"
        .encode_utf16()
        .collect();
    let identity = bytes(&volume);
    let mut root = volume;
    root.push(u16::from(b'\\'));
    check_opened_root(&root, &identity).map_err(|e| format!("{e:?}"))?;
    let mut nested = root.clone();
    nested.extend("nested".encode_utf16());
    assert_eq!(
        check_opened_root(&nested, &identity).err().map(|e| e.kind),
        Some(PortErrorKind::IdentityChanged)
    );
    root.push(u16::from(b'\\'));
    assert_eq!(
        check_opened_root(&root, &identity).err().map(|e| e.kind),
        Some(PortErrorKind::IdentityChanged)
    );
    Ok(())
}
