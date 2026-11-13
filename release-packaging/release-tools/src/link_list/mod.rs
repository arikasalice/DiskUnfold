//! Link-list check of the packaged executables (F15 release stage 4, 라이선스 관문: the link list
//! holds no LGPL or GPL item; architecture HLD 2.3.5 LGPL rule).
//!
//! The product links only system libraries that the operating system provides (DEC12). This module
//! reads the dynamic dependencies of an executable with a small parser for each format: ELF
//! `DT_NEEDED` (Linux), Mach-O `LC_LOAD_DYLIB` family (macOS, thin or universal) and PE imports
//! (Windows). Every dependency must be on that platform's allow list of system libraries, and
//! libdbus is always denied. Anything else, or a file that cannot be read, fails with
//! `release.license_denied`.

use crate::release_gate::{GateFailure, LICENSE_DENIED};

mod allow;
mod elf;
mod macho;
mod pe;
#[cfg(test)]
mod tests;

/// Test helper: a minimal ELF image that needs the given libraries.
#[cfg(test)]
pub(crate) fn elf_fixture(libraries: &[&str]) -> Vec<u8> {
    elf::fixture(libraries)
}

/// The executable format of a packaged binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// Linux ELF.
    Elf,
    /// macOS Mach-O, thin or universal.
    MachO,
    /// Windows PE.
    Pe,
}

/// Recognizes the format from the magic bytes.
pub fn detect(bytes: &[u8]) -> Option<Format> {
    match bytes.get(..4)? {
        [0x7f, b'E', b'L', b'F'] => Some(Format::Elf),
        [0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] => Some(Format::MachO),
        [b'M', b'Z', ..] => Some(Format::Pe),
        _ => None,
    }
}

/// Lists the dynamic dependencies of an executable.
///
/// # Errors
/// Returns a message when the format is not recognized or the file is malformed.
pub fn dynamic_dependencies(bytes: &[u8]) -> Result<(Format, Vec<String>), String> {
    let format = detect(bytes).ok_or("not an ELF, Mach-O or PE executable")?;
    let names = match format {
        Format::Elf => elf::needed(bytes)?,
        Format::MachO => macho::dylibs(bytes)?,
        Format::Pe => pe::imports(bytes)?,
    };
    Ok((format, names))
}

/// Checks one executable's link list. Returns the format and the number of dependencies.
///
/// # Errors
/// Returns `release.license_denied` for a dependency outside the system allow list, libdbus, or a
/// file whose link list cannot be read.
pub fn check_link_list(package: &str, bytes: &[u8]) -> Result<(Format, usize), GateFailure> {
    let denied = |detail: String| GateFailure::new(LICENSE_DENIED, format!("{package}: {detail}"));
    let (format, names) = dynamic_dependencies(bytes)
        .map_err(|error| denied(format!("the link list cannot be read ({error})")))?;
    let rejected: Vec<String> = names
        .iter()
        .filter(|name| !allow::is_system_library(format, name))
        .cloned()
        .collect();
    if rejected.is_empty() {
        Ok((format, names.len()))
    } else {
        Err(denied(format!(
            "links libraries outside the system allow list: {}",
            rejected.join(", ")
        )))
    }
}
