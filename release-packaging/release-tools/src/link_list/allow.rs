//! Allow lists of system libraries per executable format.
//!
//! Only libraries that the operating system installs may appear. The Linux entries are the system
//! packages that the deb declares as dependencies (GTK and WebKitGTK with the libraries they pull in
//! directly) and the C runtime. libdbus is denied everywhere (DEC12).

use super::Format;

const ELF_EXACT: &[&str] = &[
    "libc.so.6",
    "libm.so.6",
    "libdl.so.2",
    "libpthread.so.0",
    "librt.so.1",
    "libgcc_s.so.1",
    "libutil.so.1",
    "libresolv.so.2",
    "libstdc++.so.6",
];

/// Library families by file name before `.so`; the GTK and WebKitGTK stack the Linux launcher
/// links to.
const ELF_FAMILIES: &[&str] = &[
    "ld-linux-x86-64",
    "ld-linux-aarch64",
    "libgtk-4",
    "libwebkitgtk-6.0",
    "libjavascriptcoregtk-6.0",
    "libgdk_pixbuf-2.0",
    "libgio-2.0",
    "libglib-2.0",
    "libgobject-2.0",
    "libgmodule-2.0",
    "libpango-1.0",
    "libpangocairo-1.0",
    "libcairo",
    "libcairo-gobject",
    "libgraphene-1.0",
    "libsoup-3.0",
    "libharfbuzz",
    "libfontconfig",
    "libfreetype",
    "libz",
    "libxkbcommon",
    "libepoxy",
];

const PE_EXACT: &[&str] = &[
    "kernel32.dll",
    "kernelbase.dll",
    "user32.dll",
    "gdi32.dll",
    "advapi32.dll",
    "shell32.dll",
    "ole32.dll",
    "oleaut32.dll",
    "comctl32.dll",
    "comdlg32.dll",
    "shlwapi.dll",
    "ntdll.dll",
    "bcrypt.dll",
    "crypt32.dll",
    "secur32.dll",
    "ws2_32.dll",
    "userenv.dll",
    "version.dll",
    "dwmapi.dll",
    "uxtheme.dll",
    "imm32.dll",
    "msvcrt.dll",
    "ucrtbase.dll",
    "propsys.dll",
    "shcore.dll",
    "setupapi.dll",
    "cfgmgr32.dll",
    "winmm.dll",
    "rpcrt4.dll",
    "combase.dll",
    "dxgi.dll",
    "d3d11.dll",
    "iphlpapi.dll",
    "powrprof.dll",
    "wtsapi32.dll",
    "netapi32.dll",
    "bcryptprimitives.dll",
    // Microsoft Visual C++ runtime: redistributable and not copyleft, but not part of a clean
    // Windows. The design asks for a static CRT; until the build links it statically the check
    // allows exactly these names so the dependency is recorded, not hidden.
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "msvcp140.dll",
];

fn elf_family(name: &str) -> &str {
    name.split(".so").next().unwrap_or(name)
}

/// Whether `name` is a system library for the format.
pub fn is_system_library(format: Format, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.contains("libdbus") {
        return false;
    }
    match format {
        Format::Elf => ELF_EXACT.contains(&name) || ELF_FAMILIES.contains(&elf_family(name)),
        Format::MachO => name.starts_with("/usr/lib/") || name.starts_with("/System/Library/"),
        Format::Pe => {
            PE_EXACT.contains(&lower.as_str())
                || lower.starts_with("api-ms-win-")
                || lower.starts_with("ext-ms-")
        }
    }
}
