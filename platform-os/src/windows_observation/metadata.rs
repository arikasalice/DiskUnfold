//! Metadata of one enumerated entry and the identity of the verified handle.
use super::*;
use du_core_contracts::scan_observation::HandleIdentity;

impl WindowsDirectory {
    /// Observes one enumerated entry through a relative metadata handle.
    ///
    /// An entry on another volume is reported with that volume's serial number
    /// so that the collector can classify it outside the boundary. Its size,
    /// time and space facts are withheld, as `MetadataObservation` requires.
    ///
    /// `attributes` and `ea_size` come from the entry's enumeration record;
    /// `ea_size` is read as a reparse tag only for a reparse point.
    pub(super) fn observe(
        &self,
        name: Vec<u8>,
        inode: u64,
        attributes: u32,
        ea_size: u32,
    ) -> ObservedEntry {
        #[cfg(test)]
        PROBES.with(|count| count.set(count.get() + 1));
        let metadata = recall::observe_if_safe(attributes, || {
            wide(&name)
                .and_then(|name| relative(&self.handle, &name, false, false))
                .and_then(|file| self.metadata(&file, inode, ea_size))
        });
        ObservedEntry {
            name,
            enumerated_inode: inode,
            enumerated_kind: enumerated_kind(attributes, ea_size, &metadata),
            metadata,
        }
    }

    /// Observes a correlated child without attaching metadata from an aliased handle.
    pub(super) fn observe_correlated(
        &self,
        name: Vec<u8>,
        inode: u64,
        attributes: u32,
        ea_size: u32,
        case_insensitive: Result<bool, PortError>,
    ) -> ObservedEntry {
        #[cfg(test)]
        PROBES.with(|count| count.set(count.get() + 1));
        let metadata = recall::observe_if_safe(attributes, || {
            let file =
                relative_with_case(&self.handle, &wide(&name)?, false, false, case_insensitive?)?;
            self.metadata_for_enumerated(&file, inode, ea_size)
        });
        ObservedEntry {
            name,
            enumerated_inode: inode,
            enumerated_kind: enumerated_kind(attributes, ea_size, &metadata),
            metadata,
        }
    }

    /// Resolves a lookup mode against this directory handle and its identity.
    pub(super) fn correlated_case_insensitive(&self) -> Result<bool, PortError> {
        let case = match self.case_sensitivity() {
            Ok(case) => case,
            // Volumes without the per-directory flag (for example FAT) use case-insensitive
            // lookup. The file ID check in metadata_for_enumerated still rejects an alias.
            Err(error) if error.kind == PortErrorKind::Unsupported => return Ok(true),
            Err(error) => return Err(error),
        };
        let parent = info(&self.handle)?;
        if case.volume_serial != self.volume
            || case.volume_serial != u64::from(parent.dwVolumeSerialNumber)
            || case.file_index == 0
            || case.file_index != identity(&parent)
        {
            return Err(identity_changed());
        }
        Ok(case.mode == super::case_sensitivity::DirectoryCaseMode::Insensitive)
    }

    fn metadata(
        &self,
        file: &File,
        enumerated_inode: u64,
        ea_size: u32,
    ) -> Result<MetadataObservation, PortError> {
        let value = info(file)?;
        self.metadata_from_info(file, value, enumerated_inode, ea_size)
    }

    fn metadata_for_enumerated(
        &self,
        file: &File,
        enumerated_inode: u64,
        ea_size: u32,
    ) -> Result<MetadataObservation, PortError> {
        let value = info(file)?;
        // The enumerated 64-bit file ID is scoped to its anchored parent volume.
        if enumerated_inode == 0
            || u64::from(value.dwVolumeSerialNumber) != self.volume
            || identity(&value) != enumerated_inode
        {
            return Err(identity_changed());
        }
        self.metadata_from_info(file, value, enumerated_inode, ea_size)
    }

    fn metadata_from_info(
        &self,
        file: &File,
        value: windows::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION,
        enumerated_inode: u64,
        ea_size: u32,
    ) -> Result<MetadataObservation, PortError> {
        recall::check_handle_attributes(value.dwFileAttributes)?;
        let observed_kind = kind(value.dwFileAttributes);
        if observed_kind == ObservedKind::Link
            && reparse::has_mount_point_tag(ea_size)
            && let Some(metadata) =
                reparse::volume_root_metadata(file, &value, self.volume, enumerated_inode)?
        {
            return Ok(metadata);
        }
        let device = u64::from(value.dwVolumeSerialNumber);
        if device != self.volume {
            return Ok(other_volume(observed_kind, device, identity(&value)));
        }
        let size = (u64::from(value.nFileSizeHigh) << 32) | u64::from(value.nFileSizeLow);
        let (modified_at, native) = timestamps::observe(file);
        let (facts, native_modified_at) = if observed_kind == ObservedKind::File {
            (
                space::read(file, &value, &self.volume_guid)?,
                timestamps::matched_native(file, &value, native),
            )
        } else {
            (space::Facts::default(), None)
        };
        Ok(MetadataObservation {
            kind: observed_kind,
            device,
            inode: identity(&value),
            size: Ok(size),
            modified_at,
            native_modified_at,
            allocated: facts.allocated,
            link_count: facts.link_count,
            space_identity: facts.identity,
        })
    }

    /// Reads the identity of this handle and its retained verified ancestors.
    ///
    /// A failed query, a zero file index or a handle no longer on the opened
    /// volume yields `None`, so the collector does not enter child directories.
    pub(super) fn handle_identity(&self) -> Option<HandleIdentity> {
        let value = info(&self.handle).ok()?;
        verified_identity(
            self.volume,
            u64::from(value.dwVolumeSerialNumber),
            identity(&value),
            self.ancestors.as_ref().map(|chain| chain.ids.as_slice()),
        )
    }
}

fn enumerated_kind(
    attributes: u32,
    ea_size: u32,
    metadata: &Result<MetadataObservation, PortError>,
) -> ObservedKind {
    if metadata
        .as_ref()
        .is_ok_and(|metadata| metadata.kind == ObservedKind::MountPoint)
    {
        ObservedKind::MountPoint
    } else {
        reparse::enumerated_kind(attributes, ea_size)
    }
}

fn identity_changed() -> PortError {
    PortError {
        kind: PortErrorKind::IdentityChanged,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

/// The entry of another volume: identity and kind only, without its size.
fn other_volume(kind: ObservedKind, device: u64, inode: u64) -> MetadataObservation {
    MetadataObservation {
        kind,
        device,
        inode,
        size: Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        }),
        modified_at: None,
        native_modified_at: None,
        allocated: None,
        link_count: None,
        space_identity: None,
    }
}

/// Accepts a handle identity only on the opened volume with a nonzero index.
///
/// Retained ancestor IDs that are not 8 bytes cannot be compared with the
/// 64-bit index and are omitted.
fn verified_identity(
    opened_volume: u64,
    device: u64,
    inode: u64,
    ancestors: Option<&[Vec<u8>]>,
) -> Option<HandleIdentity> {
    if inode == 0 || device != opened_volume {
        return None;
    }
    let ancestors = ancestors.map_or_else(Vec::new, |ids| {
        ids.iter()
            .filter_map(|id| <[u8; 8]>::try_from(id.as_slice()).ok())
            .map(u64::from_le_bytes)
            .collect()
    });
    Some(HandleIdentity {
        device,
        inode,
        ancestors,
    })
}

#[cfg(test)]
mod tests;
