//! Native read-only protection facts used while building plans (F29).

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use du_core_contracts::{
    PathEncoding,
    protection_facts::{
        KnownLocationFact, KnownLocationId, LinkBoundaryFact, ObservedFileIdentity,
        ObservedProtectionItem, ProtectionFact, ProtectionFactsPort, ProtectionGuardFact,
        ProtectionOperation, VolumeRoleFact, fact_reason,
    },
    ruleset::KnownLocationTableVersion,
};

#[cfg(unix)]
use std::{ffi::OsString, os::unix::ffi::OsStringExt};
#[cfg(windows)]
use std::{
    ffi::OsString,
    os::windows::{ffi::OsStringExt, fs::MetadataExt},
};

#[cfg(unix)]
#[allow(unsafe_code)]
mod unix;
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

#[cfg(windows)]
mod native {
    pub(super) use super::windows::{
        disk_backed, identity, link_boundary, os_component, permission, volume_role,
    };
}
#[cfg(unix)]
mod native {
    pub(super) use super::unix::{
        disk_backed, identity, link_boundary, os_component, permission, volume_role,
    };
}

const IDENTITY_UNKNOWN: &str = fact_reason::UNSUPPORTED;
const LINK_UNKNOWN: &str = fact_reason::UNSUPPORTED;
const ROLE_UNKNOWN: &str = fact_reason::UNSUPPORTED;
const LOCATION_UNKNOWN: &str = fact_reason::PROVIDER_ERROR;
const COMPONENT_UNKNOWN: &str = fact_reason::UNSUPPORTED;
const AGE_UNKNOWN: &str = fact_reason::PARTIAL;
const PERMISSION_UNKNOWN: &str = fact_reason::PROVIDER_ERROR;
const DISK_UNKNOWN: &str = fact_reason::UNSUPPORTED;

/// Read-only protection facts backed by the current operating system.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProtectionFacts;

impl ProtectionFactsPort for SystemProtectionFacts {
    fn identity(&self, item: &ObservedProtectionItem) -> ProtectionFact<ObservedFileIdentity> {
        let Some(path) = native_path(&item.path) else {
            return ProtectionFact::Unknown(IDENTITY_UNKNOWN);
        };
        native::identity(&path).map_or(
            ProtectionFact::Unknown(IDENTITY_UNKNOWN),
            ProtectionFact::Known,
        )
    }

    fn link_boundary(&self, item: &ObservedProtectionItem) -> ProtectionFact<LinkBoundaryFact> {
        let Some(path) = native_path(&item.path) else {
            return ProtectionFact::Unknown(LINK_UNKNOWN);
        };
        native::link_boundary(item, &path)
            .map_or(ProtectionFact::Unknown(LINK_UNKNOWN), ProtectionFact::Known)
    }

    fn volume_role(&self, item: &ObservedProtectionItem) -> ProtectionFact<VolumeRoleFact> {
        let Some(path) = native_path(&item.path) else {
            return ProtectionFact::Unknown(ROLE_UNKNOWN);
        };
        native::volume_role(&path)
            .map_or(ProtectionFact::Unknown(ROLE_UNKNOWN), ProtectionFact::Known)
    }

    fn known_location(
        &self,
        item: &ObservedProtectionItem,
        table_version: KnownLocationTableVersion,
    ) -> KnownLocationFact {
        if table_version != KnownLocationTableVersion::V1 {
            return KnownLocationFact::Unknown(fact_reason::STALE_TABLE);
        }
        let Some(path) = native_path(&item.path) else {
            return KnownLocationFact::Unknown(LOCATION_UNKNOWN);
        };
        match known_location(&path) {
            Ok(Some(location)) => KnownLocationFact::Listed(location),
            Ok(None) => KnownLocationFact::NotListed,
            Err(reason) => KnownLocationFact::Unknown(reason),
        }
    }

    fn os_component(&self, item: &ObservedProtectionItem) -> ProtectionFact<bool> {
        let Some(path) = native_path(&item.path) else {
            return ProtectionFact::Unknown(COMPONENT_UNKNOWN);
        };
        if native::os_component(&path) {
            return ProtectionFact::Known(true);
        }
        match known_location(&path) {
            Ok(Some(_)) => ProtectionFact::Known(false),
            Ok(None) => ProtectionFact::Unknown(COMPONENT_UNKNOWN),
            Err(reason) => ProtectionFact::Unknown(reason),
        }
    }

    fn guard_age(&self, item: &ObservedProtectionItem) -> ProtectionFact<ProtectionGuardFact> {
        #[cfg(windows)]
        {
            let Some(path) = native_path(&item.path) else {
                return ProtectionFact::Unknown(AGE_UNKNOWN);
            };
            let location = match known_location(&path) {
                Ok(location) => location,
                Err(reason) => return ProtectionFact::Unknown(reason),
            };
            if location != Some(KnownLocationId::Temp) {
                return ProtectionFact::Known(ProtectionGuardFact::Clear);
            }
            let Some(modified_at) = item.modified_at.as_ref() else {
                return ProtectionFact::Unknown(AGE_UNKNOWN);
            };
            let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
                return ProtectionFact::Unknown(AGE_UNKNOWN);
            };
            let now_ms = i128::from(now.as_millis().min(i128::MAX as u128) as i64);
            let age_ms = now_ms - i128::from(modified_at.unix_millis());
            let threshold_ms = 7_i128 * 24 * 60 * 60 * 1000;
            if age_ms < 0 {
                ProtectionFact::Unknown(AGE_UNKNOWN)
            } else if age_ms < threshold_ms {
                ProtectionFact::Known(ProtectionGuardFact::Protected)
            } else {
                ProtectionFact::Known(ProtectionGuardFact::Clear)
            }
        }
        #[cfg(not(windows))]
        {
            if item.modified_at.is_some() {
                ProtectionFact::Known(ProtectionGuardFact::Clear)
            } else {
                ProtectionFact::Unknown(AGE_UNKNOWN)
            }
        }
    }

    fn guard_permission(
        &self,
        item: &ObservedProtectionItem,
        operation: ProtectionOperation,
    ) -> ProtectionFact<ProtectionGuardFact> {
        let Some(path) = native_path(&item.path) else {
            return ProtectionFact::Unknown(PERMISSION_UNKNOWN);
        };
        native::permission(&path, operation).map_or(
            ProtectionFact::Unknown(PERMISSION_UNKNOWN),
            ProtectionFact::Known,
        )
    }

    fn guard_disk_backed(
        &self,
        item: &ObservedProtectionItem,
    ) -> ProtectionFact<ProtectionGuardFact> {
        let Some(path) = native_path(&item.path) else {
            return ProtectionFact::Unknown(DISK_UNKNOWN);
        };
        native::disk_backed(&path)
            .map_or(ProtectionFact::Unknown(DISK_UNKNOWN), ProtectionFact::Known)
    }
}

fn native_path(path: &du_core_contracts::OsPath) -> Option<PathBuf> {
    match path.encoding {
        PathEncoding::Unix => {
            #[cfg(unix)]
            {
                Some(PathBuf::from(OsString::from_vec(
                    path.raw.bytes().ok()?.to_vec(),
                )))
            }
            #[cfg(not(unix))]
            {
                None
            }
        }
        PathEncoding::WindowsUtf16le => {
            #[cfg(windows)]
            {
                let bytes = path.raw.bytes().ok()?;
                let (units, tail) = bytes.as_chunks::<2>();
                if !tail.is_empty() {
                    return None;
                }
                let wide = units
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect::<Vec<_>>();
                (!wide.contains(&0)).then(|| PathBuf::from(OsString::from_wide(&wide)))
            }
            #[cfg(not(windows))]
            {
                None
            }
        }
    }
}

fn known_location(path: &std::path::Path) -> Result<Option<KnownLocationId>, &'static str> {
    if has_link_component(path).ok_or(LOCATION_UNKNOWN)? {
        return Err(LOCATION_UNKNOWN);
    }
    let target = path
        .canonicalize()
        .map_err(|error| io_fact_reason(&error))?;
    let roots = location_roots().ok_or(LOCATION_UNKNOWN)?;
    let mut best: Option<(usize, KnownLocationId)> = None;
    for (root, location) in roots {
        match root.canonicalize() {
            Ok(root) if target.starts_with(&root) => {
                let depth = root.components().count();
                if best.is_none_or(|(best_depth, _)| depth > best_depth) {
                    best = Some((depth, location));
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_fact_reason(&error)),
        }
    }
    Ok(best.map(|(_, location)| location))
}

fn io_fact_reason(error: &std::io::Error) -> &'static str {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::PermissionDenied => fact_reason::ACCESS_DENIED,
        ErrorKind::TimedOut => fact_reason::TIMED_OUT,
        ErrorKind::Interrupted => fact_reason::CANCELLED,
        _ => fact_reason::PROVIDER_ERROR,
    }
}

fn location_roots() -> Option<Vec<(PathBuf, KnownLocationId)>> {
    let home = home_dir()?;
    let mut roots = vec![(home.clone(), KnownLocationId::UserHome)];
    roots.push((home.join("Downloads"), KnownLocationId::Downloads));
    #[cfg(windows)]
    {
        if let Some(path) = std::env::var_os("APPDATA").map(PathBuf::from) {
            roots.push((path, KnownLocationId::AppData));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            roots.push((local.join("Cache"), KnownLocationId::UserCache));
            roots.push((local, KnownLocationId::AppData));
        }
        roots.push((std::env::temp_dir(), KnownLocationId::Temp));
        if let Some(drive_root) = home.ancestors().last() {
            roots.push((drive_root.join("$Recycle.Bin"), KnownLocationId::TrashStore));
        }
    }
    #[cfg(target_os = "macos")]
    {
        roots.push((
            home.join("Library/Application Support"),
            KnownLocationId::AppData,
        ));
        roots.push((home.join("Library/Caches"), KnownLocationId::UserCache));
        roots.push((std::env::temp_dir(), KnownLocationId::Temp));
        roots.push((home.join(".Trash"), KnownLocationId::TrashStore));
    }
    #[cfg(target_os = "linux")]
    {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"));
        roots.push((data, KnownLocationId::AppData));
        roots.push((cache, KnownLocationId::UserCache));
        roots.push((std::env::temp_dir(), KnownLocationId::Temp));
        roots.push((home.join(".local/share/Trash"), KnownLocationId::TrashStore));
        roots.push((PathBuf::from("/dev/shm"), KnownLocationId::MemoryManaged));
    }
    Some(roots)
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(unix)]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn has_link_component(path: &std::path::Path) -> Option<bool> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).ok()?;
        if metadata.file_type().is_symlink() {
            return Some(true);
        }
        #[cfg(windows)]
        if metadata.file_attributes()
            & ::windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT.0
            != 0
        {
            return Some(true);
        }
    }
    Some(false)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use du_core_contracts::{OsPath, PathBytes, PathEncoding, result::ResultItemKind};
    use std::{
        os::windows::ffi::OsStrExt,
        path::{Path, PathBuf},
        process::Command,
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn observed(path: &Path, root: &Path) -> ObservedProtectionItem {
        ObservedProtectionItem {
            path: os_path(path),
            scan_root_path: Some(os_path(root)),
            kind: ResultItemKind::File,
            volume_id: Vec::new(),
            identity: None,
            modified_at: None,
        }
    }

    fn os_path(path: &Path) -> OsPath {
        let bytes = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        OsPath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&bytes),
            display: path.to_string_lossy().into_owned(),
            lossy: false,
        }
    }

    #[test]
    fn repeated_identity_reads_match_for_a_real_temporary_file() -> TestResult {
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("identity.txt");
        fs::write(&file, b"identity")?;
        let first = SystemProtectionFacts.identity(&observed(&file, directory.path()));
        let second = SystemProtectionFacts.identity(&observed(&file, directory.path()));
        assert!(
            matches!((first, second), (ProtectionFact::Known(left), ProtectionFact::Known(right)) if left == right)
        );
        Ok(())
    }

    #[test]
    fn junction_is_a_link_boundary_and_completed_lookup_returns_not_listed() -> TestResult {
        let directory = tempfile::tempdir()?;
        let root = directory.path().join("root");
        let outside = directory.path().join("outside");
        fs::create_dir(&root)?;
        fs::create_dir(&outside)?;
        fs::write(outside.join("target.txt"), b"target")?;
        let junction = root.join("junction");
        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()?;
        if !output.status.success() {
            eprintln!(
                "junction creation unavailable: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return Ok(());
        }
        let facts = SystemProtectionFacts;
        let item = observed(&junction.join("target.txt"), &root);
        assert_eq!(
            facts.link_boundary(&item),
            ProtectionFact::Known(LinkBoundaryFact::OutsideVolume)
        );

        let system_root =
            PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot missing")?);
        let system_file = system_root.join("System32").join("kernel32.dll");
        assert_eq!(
            facts.known_location(
                &observed(&system_file, &system_root),
                KnownLocationTableVersion::V1,
            ),
            KnownLocationFact::NotListed
        );
        assert!(matches!(
            facts.known_location(
                &observed(&directory.path().join("missing"), &root),
                KnownLocationTableVersion::V1,
            ),
            KnownLocationFact::Unknown(_)
        ));
        Ok(())
    }
}
