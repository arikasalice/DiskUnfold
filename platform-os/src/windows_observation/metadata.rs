//! Metadata of one enumerated entry and the identity of the verified handle.
use super::*;
use du_core_contracts::scan_observation::HandleIdentity;

impl WindowsDirectory {
    /// Observes one enumerated entry through a relative metadata handle.
    ///
    /// An entry on another volume is reported with that volume's serial number
    /// so that the collector can classify it outside the boundary. No time or
    /// space facts are read from that volume.
    pub(super) fn observe(&self, name: Vec<u8>, inode: u64, attributes: u32) -> ObservedEntry {
        #[cfg(test)]
        PROBES.with(|count| count.set(count.get() + 1));
        let metadata = recall::observe_if_safe(attributes, || {
            wide(&name)
                .and_then(|name| relative(&self.handle, &name, false, false))
                .and_then(|file| self.metadata(&file))
        });
        ObservedEntry {
            name,
            enumerated_inode: inode,
            enumerated_kind: kind(attributes),
            metadata,
        }
    }

    fn metadata(&self, file: &File) -> Result<MetadataObservation, PortError> {
        let value = info(file)?;
        recall::check_handle_attributes(value.dwFileAttributes)?;
        let size = (u64::from(value.nFileSizeHigh) << 32) | u64::from(value.nFileSizeLow);
        let observed_kind = kind(value.dwFileAttributes);
        let device = u64::from(value.dwVolumeSerialNumber);
        if device != self.volume {
            return Ok(MetadataObservation {
                kind: observed_kind,
                device,
                inode: identity(&value),
                size: Ok(size),
                modified_at: None,
                native_modified_at: None,
                allocated: None,
                link_count: None,
                space_identity: None,
            });
        }
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
        let device = u64::from(value.dwVolumeSerialNumber);
        let inode = identity(&value);
        if inode == 0 || device != self.volume {
            return None;
        }
        let ancestors = self.ancestors.as_ref().map_or_else(Vec::new, |chain| {
            chain
                .ids
                .iter()
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
}

#[cfg(test)]
mod tests;
