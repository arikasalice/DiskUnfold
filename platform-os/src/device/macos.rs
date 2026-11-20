//! macOS volume and IOKit media observations.
use super::{
    DeviceProbe, OS_DEVICE_MULTIPLE_DISKS, OS_DEVICE_NETWORK, OS_QUERY_FAILED, OS_UNSUPPORTED,
    ProbedDeviceKind,
};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    os::fd::{AsRawFd, BorrowedFd},
    ptr,
};

type IoService = u32;
type CfType = *const c_void;
type CfString = *const c_void;
const IO_REGISTRY_ITERATE_RECURSIVELY: u32 = 1;
const IO_REGISTRY_ITERATE_PARENTS: u32 = 2;
const IO_REGISTRY_RECURSIVE_PARENTS: u32 =
    IO_REGISTRY_ITERATE_RECURSIVELY | IO_REGISTRY_ITERATE_PARENTS;
const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const IO_SERVICE_PLANE: &CStr = c"IOService";
const IO_MEDIA_CLASS: &CStr = c"IOMedia";
const APFS_CONTAINER_SCHEME_CLASS: &CStr = c"AppleAPFSContainerScheme";

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOBSDNameMatching(master_port: u32, bsd_name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(master_port: u32, matching: *mut c_void) -> IoService;
    fn IORegistryEntrySearchCFProperty(
        entry: IoService,
        plane: *const c_char,
        key: CfType,
        allocator: CfType,
        options: u32,
    ) -> CfType;
    fn IORegistryEntryGetParentEntry(
        entry: IoService,
        plane: *const c_char,
        parent: *mut IoService,
    ) -> i32;
    fn IORegistryEntryGetParentIterator(
        entry: IoService,
        plane: *const c_char,
        iterator: *mut IoService,
    ) -> i32;
    fn IOIteratorNext(iterator: IoService) -> IoService;
    fn IOObjectConformsTo(object: IoService, class_name: *const c_char) -> u32;
    fn IOObjectRelease(object: IoService) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        allocator: CfType,
        value: *const c_char,
        encoding: u32,
    ) -> CfString;
    fn CFDictionaryGetValue(dictionary: CfType, key: CfType) -> CfType;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFBooleanGetValue(value: CfType) -> u8;
    fn CFBooleanGetTypeID() -> usize;
    fn CFGetTypeID(value: CfType) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFStringGetCString(
        value: CfString,
        buffer: *mut c_char,
        capacity: isize,
        encoding: u32,
    ) -> u8;
    fn CFRelease(value: CfType);
}

/// Probes the local storage volume containing an already-open root directory.
pub fn probe_fd(fd: BorrowedFd<'_>) -> DeviceProbe {
    // SAFETY: statfs is initialized by the OS and fd remains borrowed for the call.
    let mut stats = unsafe { std::mem::zeroed::<libc::statfs>() };
    // SAFETY: stats is writable and fd is an open descriptor supplied by the caller.
    if unsafe { libc::fstatfs(fd.as_raw_fd(), &mut stats) } != 0 {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
    }
    if stats.f_flags & (libc::MNT_LOCAL as u32) == 0 {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_DEVICE_NETWORK));
    }

    // SAFETY: fstatfs writes a NUL-terminated mount source into the fixed-size field.
    let source = unsafe { CStr::from_ptr(stats.f_mntfromname.as_ptr()) }.to_bytes();
    let Some(bsd_name) = source.strip_prefix(b"/dev/") else {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_UNSUPPORTED));
    };
    if !bsd_name.starts_with(b"disk")
        || bsd_name.is_empty()
        || !bsd_name.iter().all(u8::is_ascii_alphanumeric)
    {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_UNSUPPORTED));
    }
    let Ok(bsd_name) = CString::new(bsd_name) else {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_UNSUPPORTED));
    };

    // SAFETY: bsd_name is NUL-terminated; IOBSDNameMatching returns a matching dictionary.
    let matching = unsafe { IOBSDNameMatching(0, bsd_name.as_ptr()) };
    if matching.is_null() {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
    }
    // SAFETY: the matching dictionary is consumed by IOServiceGetMatchingService.
    let service = unsafe { IOServiceGetMatchingService(0, matching) };
    if service == 0 {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
    }

    inspect_media(service)
}

fn inspect_media(service: IoService) -> DeviceProbe {
    let start_kind = match medium_kind(service) {
        Ok(kind) => kind,
        Err(()) => return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED)),
    };
    let mut current = IoObject(service);
    let mut candidate_disk = None;
    let mut ambiguous = false;
    let mut chain_complete = false;
    for _ in 0..128 {
        if conforms(current.0, APFS_CONTAINER_SCHEME_CLASS) {
            return inspect_apfs_container(current.0, start_kind);
        }
        let candidate = match whole_disk(current.0) {
            Ok(candidate) => candidate,
            Err(()) => return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED)),
        };
        if let Some(candidate) = candidate {
            if candidate_disk
                .as_ref()
                .is_some_and(|(existing, _)| existing != &candidate.0)
            {
                ambiguous = true;
            } else {
                candidate_disk = Some(candidate);
            }
        }

        let mut parent = 0;
        // SAFETY: the output pointer is writable; a successful result transfers a retained parent.
        if unsafe {
            IORegistryEntryGetParentEntry(current.0, IO_SERVICE_PLANE.as_ptr(), &mut parent)
        } != 0
        {
            chain_complete = true;
            break;
        }
        if parent == 0 {
            chain_complete = true;
            break;
        }
        current = IoObject(parent);
    }
    if !chain_complete {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
    }
    if ambiguous {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
    }
    let Some((name, disk_kind)) = candidate_disk else {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
    };
    resolved_probe(name, disk_kind, start_kind)
}

fn whole_disk(entry: IoService) -> Result<Option<(String, ProbedDeviceKind)>, ()> {
    if !conforms(entry, IO_MEDIA_CLASS) {
        return Ok(None);
    }
    let Some(true) = property_bool(entry, "Whole")? else {
        return Ok(None);
    };
    let Some(name) = property_string(entry, "BSD Name")? else {
        return Err(());
    };
    let kind = match medium_kind(entry)? {
        Some(kind @ (ProbedDeviceKind::Ssd | ProbedDeviceKind::Hdd)) => kind,
        None | Some(ProbedDeviceKind::Unknown) => ProbedDeviceKind::Unknown,
    };
    Ok(Some((name, kind)))
}

fn inspect_apfs_container(scheme: IoService, start_kind: Option<ProbedDeviceKind>) -> DeviceProbe {
    let mut iterator = 0;
    // APFS exposes each physical store as a parent IOMedia of its container scheme.
    // SAFETY: iterator is writable and scheme is a retained registry object.
    if unsafe { IORegistryEntryGetParentIterator(scheme, IO_SERVICE_PLANE.as_ptr(), &mut iterator) }
        != 0
        || iterator == 0
    {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
    }
    let iterator = IoObject(iterator);
    let mut stores = Vec::new();
    loop {
        // SAFETY: iterator is a retained IOKit iterator; each returned object is retained.
        let store = unsafe { IOIteratorNext(iterator.0) };
        if store == 0 {
            break;
        }
        if !conforms(store, IO_MEDIA_CLASS) {
            // SAFETY: IOIteratorNext returned an owned object.
            unsafe { IOObjectRelease(store) };
            return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
        }
        stores.push(IoObject(store));
        if stores.len() > 1 {
            return probe(
                ProbedDeviceKind::Unknown,
                None,
                Some(OS_DEVICE_MULTIPLE_DISKS),
            );
        }
    }
    let Some(store) = stores.into_iter().next() else {
        return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
    };
    let (name, disk_kind) = match physical_whole_disk(store) {
        Ok(Some(disk)) => disk,
        Ok(None) | Err(()) => {
            return probe(ProbedDeviceKind::Unknown, None, Some(OS_QUERY_FAILED));
        }
    };
    resolved_probe(name, disk_kind, start_kind)
}

fn physical_whole_disk(mut current: IoObject) -> Result<Option<(String, ProbedDeviceKind)>, ()> {
    let mut chain_complete = false;
    for _ in 0..128 {
        if let Some(disk) = whole_disk(current.0)? {
            return Ok(Some(disk));
        }
        let mut parent = 0;
        // SAFETY: the output pointer is writable; a successful result transfers a retained parent.
        if unsafe {
            IORegistryEntryGetParentEntry(current.0, IO_SERVICE_PLANE.as_ptr(), &mut parent)
        } != 0
            || parent == 0
        {
            chain_complete = true;
            break;
        }
        current = IoObject(parent);
    }
    if chain_complete { Ok(None) } else { Err(()) }
}

fn conforms(entry: IoService, class_name: &CStr) -> bool {
    // SAFETY: entry is retained and class_name is NUL-terminated.
    unsafe { IOObjectConformsTo(entry, class_name.as_ptr()) != 0 }
}

fn resolved_probe(
    name: String,
    disk_kind: ProbedDeviceKind,
    volume_kind: Option<ProbedDeviceKind>,
) -> DeviceProbe {
    let kind = match (volume_kind, disk_kind) {
        (_, ProbedDeviceKind::Unknown) => ProbedDeviceKind::Unknown,
        (Some(volume @ (ProbedDeviceKind::Ssd | ProbedDeviceKind::Hdd)), disk)
            if volume != disk =>
        {
            ProbedDeviceKind::Unknown
        }
        (Some(volume @ (ProbedDeviceKind::Ssd | ProbedDeviceKind::Hdd)), _) => volume,
        (_, disk) => disk,
    };
    let key = format!("macos:disk:{name}").into_bytes();
    let reason = (kind == ProbedDeviceKind::Unknown).then_some(OS_QUERY_FAILED);
    probe(kind, Some(key), reason)
}

struct IoObject(IoService);

impl Drop for IoObject {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns a retained IOKit object.
        unsafe { IOObjectRelease(self.0) };
    }
}

struct CfObject(CfType);

impl Drop for CfObject {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns a retained CoreFoundation object.
        unsafe { CFRelease(self.0) };
    }
}

fn property(entry: IoService, name: &str, options: u32) -> Result<Option<CfObject>, ()> {
    let key_name = CString::new(name).map_err(|_| ())?;
    // SAFETY: the NUL-terminated key and static plane are valid; null allocator selects default.
    let key = unsafe {
        CFStringCreateWithCString(ptr::null(), key_name.as_ptr(), CF_STRING_ENCODING_UTF8)
    };
    if key.is_null() {
        return Err(());
    }
    let key = CfObject(key);
    // SAFETY: entry is retained and key remains alive during the property query.
    let value = unsafe {
        IORegistryEntrySearchCFProperty(
            entry,
            IO_SERVICE_PLANE.as_ptr(),
            key.0,
            ptr::null(),
            options,
        )
    };
    Ok((!value.is_null()).then_some(CfObject(value)))
}

fn property_bool(entry: IoService, name: &str) -> Result<Option<bool>, ()> {
    let Some(value) = property(entry, name, 0)? else {
        return Ok(None);
    };
    // SAFETY: value is a retained CoreFoundation value and type IDs are valid queries.
    if unsafe { CFGetTypeID(value.0) } != unsafe { CFBooleanGetTypeID() } {
        return Err(());
    }
    // SAFETY: the type check above established a CFBoolean value.
    Ok(Some(unsafe { CFBooleanGetValue(value.0) != 0 }))
}

fn property_string(entry: IoService, name: &str) -> Result<Option<String>, ()> {
    let Some(value) = property(entry, name, 0)? else {
        return Ok(None);
    };
    cf_string(value.0).map(Some)
}

fn medium_kind(entry: IoService) -> Result<Option<ProbedDeviceKind>, ()> {
    let Some(dictionary) = property(
        entry,
        "Device Characteristics",
        IO_REGISTRY_RECURSIVE_PARENTS,
    )?
    else {
        return Ok(None);
    };
    // SAFETY: dictionary is a retained CoreFoundation value.
    if unsafe { CFGetTypeID(dictionary.0) } != unsafe { CFDictionaryGetTypeID() } {
        return Err(());
    }
    let name = CString::new("Medium Type").map_err(|_| ())?;
    // SAFETY: the dictionary and NUL-terminated key are live for the lookup.
    let key =
        unsafe { CFStringCreateWithCString(ptr::null(), name.as_ptr(), CF_STRING_ENCODING_UTF8) };
    if key.is_null() {
        return Err(());
    }
    let key = CfObject(key);
    // SAFETY: dictionary is typed and both values remain live for this lookup.
    let value = unsafe { CFDictionaryGetValue(dictionary.0, key.0) };
    if value.is_null() {
        return Ok(None);
    }
    let medium = cf_string(value)?;
    Ok(Some(match medium.as_str() {
        "Solid State" => ProbedDeviceKind::Ssd,
        "Rotational" => ProbedDeviceKind::Hdd,
        _ => ProbedDeviceKind::Unknown,
    }))
}

fn cf_string(value: CfType) -> Result<String, ()> {
    // SAFETY: value is a retained CF object; type IDs are valid CoreFoundation queries.
    if unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
        return Err(());
    }
    let mut buffer = [0i8; 128];
    // SAFETY: buffer is writable, and CFStringGetCString verifies the source type.
    if unsafe {
        CFStringGetCString(
            value,
            buffer.as_mut_ptr(),
            buffer.len() as isize,
            CF_STRING_ENCODING_UTF8,
        )
    } == 0
    {
        return Err(());
    }
    // SAFETY: successful CFStringGetCString writes a NUL-terminated UTF-8 string.
    let value = unsafe { CStr::from_ptr(buffer.as_ptr()) };
    String::from_utf8(value.to_bytes().to_vec()).map_err(|_| ())
}

fn probe(
    kind: ProbedDeviceKind,
    disk_key: Option<Vec<u8>>,
    reason_key: Option<&'static str>,
) -> DeviceProbe {
    DeviceProbe {
        kind,
        disk_key,
        reason_key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{error::Error, fs::File, io, os::fd::AsFd};

    #[test]
    fn unknown_disk_kind_keeps_whole_disk_key() -> Result<(), Box<dyn Error>> {
        let result = resolved_probe(
            "disk0".to_owned(),
            ProbedDeviceKind::Unknown,
            Some(ProbedDeviceKind::Ssd),
        );
        if result.kind != ProbedDeviceKind::Unknown
            || result.disk_key.as_deref() != Some(b"macos:disk:disk0")
            || result.reason_key != Some(OS_QUERY_FAILED)
        {
            return Err(io::Error::other("unknown kind did not retain the disk key").into());
        }
        Ok(())
    }

    #[test]
    fn macos_open_root_device_smoke() -> Result<(), Box<dyn Error>> {
        let root = File::open("/")?;
        let result = probe_fd(root.as_fd());
        eprintln!("macOS open-root device probe: {result:?}");
        if result
            .disk_key
            .as_deref()
            .is_some_and(|key| !key.starts_with(b"macos:disk:"))
        {
            return Err(io::Error::other("macOS disk key has an unexpected prefix").into());
        }
        Ok(())
    }
}
