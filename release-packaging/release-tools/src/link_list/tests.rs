use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn elf_needed_entries_are_read_through_the_virtual_address_translation() -> TestResult {
    let image = elf::fixture(&["libgtk-4.so.1", "libc.so.6"]);
    assert_eq!(elf::needed(&image)?, ["libgtk-4.so.1", "libc.so.6"]);
    let (format, names) = dynamic_dependencies(&image)?;
    assert_eq!((format, names.len()), (Format::Elf, 2));
    Ok(())
}

#[test]
fn mach_o_thin_and_universal_images_are_read() -> TestResult {
    let arm = macho::fixture(&["/usr/lib/libSystem.B.dylib"]);
    let intel = macho::fixture(&["/usr/lib/libobjc.A.dylib", "/opt/homebrew/lib/libfoo.dylib"]);
    assert_eq!(macho::dylibs(&arm)?, ["/usr/lib/libSystem.B.dylib"]);
    let fat = macho::universal(&[arm, intel]);
    assert_eq!(macho::dylibs(&fat)?.len(), 3);
    Ok(())
}

#[test]
fn pe_imports_are_read() -> TestResult {
    let image = pe::fixture(&["KERNEL32.dll", "user32.dll"]);
    assert_eq!(pe::imports(&image)?, ["KERNEL32.dll", "user32.dll"]);
    Ok(())
}

#[test]
fn system_libraries_pass_on_every_format() -> TestResult {
    let elf = elf::fixture(&[
        "libc.so.6",
        "libgtk-4.so.1",
        "libwebkitgtk-6.0.so.4",
        "ld-linux-x86-64.so.2",
    ]);
    assert_eq!(check_link_list("pkg", &elf)?.1, 4);
    let mac = macho::universal(&[macho::fixture(&[
        "/usr/lib/libSystem.B.dylib",
        "/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit",
    ])]);
    assert_eq!(check_link_list("pkg", &mac)?.0, Format::MachO);
    let pe = pe::fixture(&["KERNEL32.dll", "api-ms-win-core-synch-l1-2-0.dll"]);
    assert_eq!(check_link_list("pkg", &pe)?.0, Format::Pe);
    Ok(())
}

#[test]
fn a_library_outside_the_allow_list_is_denied_with_the_design_key() -> TestResult {
    for image in [
        elf::fixture(&["libc.so.6", "libdbus-1.so.3"]),
        elf::fixture(&["libreadline.so.8"]),
        macho::fixture(&["/opt/homebrew/lib/libfoo.dylib"]),
        macho::fixture(&["@rpath/libfoo.dylib"]),
        pe::fixture(&["KERNEL32.dll", "libgcc_s_seh-1.dll"]),
    ] {
        let failure = check_link_list("pkg", &image)
            .err()
            .ok_or("must be denied")?;
        assert_eq!(failure.reason_key, "release.license_denied");
        assert!(
            failure.detail.contains("outside the system allow list"),
            "{failure}"
        );
    }
    Ok(())
}

#[test]
fn a_renamed_or_unreadable_binary_fails_closed() -> TestResult {
    for bytes in [
        b"plain text".to_vec(),
        b"\x7fELF".to_vec(),
        b"MZ".to_vec(),
        vec![0xcf, 0xfa, 0xed, 0xfe],
    ] {
        let failure = check_link_list("pkg", &bytes).err().ok_or("must fail")?;
        assert_eq!(failure.reason_key, "release.license_denied");
        assert!(failure.detail.contains("cannot be read"), "{failure}");
    }
    // A truncated dynamic section is not read as an empty list.
    let mut image = elf::fixture(&["libc.so.6"]);
    image.truncate(image.len() - 4);
    assert!(elf::needed(&image).is_err());
    Ok(())
}

#[test]
fn a_32_bit_or_big_endian_elf_is_not_guessed_at() {
    let mut image = elf::fixture(&["libc.so.6"]);
    image[4] = 1;
    assert!(elf::needed(&image).is_err());
}

#[test]
fn a_statically_linked_elf_has_no_dependencies() -> TestResult {
    let mut image = elf::fixture(&[]);
    // Turn the dynamic program header into a note so the file has no dynamic section.
    image[64 + 56..64 + 60].copy_from_slice(&4_u32.to_le_bytes());
    assert_eq!(check_link_list("pkg", &image)?.1, 0);
    Ok(())
}
