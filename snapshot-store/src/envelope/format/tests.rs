use super::*;

struct Versions(&'static [u16]);

impl SnapshotFormatRegistry for Versions {
    fn is_registered(&self, version: u16) -> bool {
        self.0.contains(&version)
    }
}

/// A complete version 1 file; `version` replaces the header field.
fn file(version: u16) -> Vec<u8> {
    let (metadata, body) = (b"{}".as_slice(), b"body".as_slice());
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    bytes.extend_from_slice(metadata);
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(&(body.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(blake3::hash(&bytes).as_bytes());
    bytes[8..10].copy_from_slice(&version.to_le_bytes());
    bytes
}

fn unknown(version: u16) -> OpenError {
    OpenError::FormatUnknown(FormatUnknown { version })
}

#[test]
fn the_current_version_dispatches_to_its_reader() -> Result<(), OpenError> {
    let bytes = file(DUSNAP_FORMAT_VERSION);
    let registry = Versions(&[DUSNAP_FORMAT_VERSION]);
    assert_eq!(select_reader(&bytes, &registry)?, Reader::V1);
    let view = open_verified(&bytes, &registry)?;
    assert_eq!(view.version, DUSNAP_FORMAT_VERSION);
    assert_eq!(view.compressed, b"body");
    Ok(())
}

#[test]
fn every_reader_in_the_table_is_selected_by_its_own_version() {
    // Only version 1 exists today, so there is no older supported version to open. A second
    // reader module extends `READERS`; this checks that the table maps version to reader.
    for (version, reader) in READERS {
        assert_eq!(reader_for(*version), Some(*reader));
    }
    assert_eq!(READERS.len(), 1);
}

#[test]
fn newer_and_unregistered_versions_fail_closed_with_the_published_key_and_change_nothing() {
    for version in [0, DUSNAP_FORMAT_VERSION + 1, 2, 255, u16::MAX] {
        let mut bytes = file(version);
        // A newer format may lay out the flags and footer differently; they are not read.
        bytes[10] = 0xff;
        let before = bytes.clone();
        // Registered by the composition root but without a reader in this build: still unknown.
        let registry = Versions(&[0, 2, 255, u16::MAX]);
        let error = open_verified(&bytes, &registry).err();
        assert_eq!(error, Some(unknown(version)), "version {version}");
        assert_eq!(
            error.and_then(|error| error.reason_key()),
            Some("snapshot.format_unknown")
        );
        assert_eq!(bytes, before);
    }
}

#[test]
fn a_registry_without_the_current_version_makes_the_file_unreadable() {
    let bytes = file(DUSNAP_FORMAT_VERSION);
    assert_eq!(
        open_verified(&bytes, &Versions(&[])).err(),
        Some(unknown(DUSNAP_FORMAT_VERSION))
    );
}

#[test]
fn one_unreadable_file_does_not_affect_the_next() -> Result<(), OpenError> {
    let registry = Versions(&[DUSNAP_FORMAT_VERSION]);
    assert!(open_verified(&file(2), &registry).is_err());
    assert_eq!(open_verified(&file(1), &registry)?.version, 1);
    Ok(())
}

#[test]
fn a_corrupt_header_is_not_reported_as_an_unknown_format() {
    let registry = Versions(&[DUSNAP_FORMAT_VERSION]);
    let good = file(DUSNAP_FORMAT_VERSION);
    for end in 0..VERSION_PREFIX_BYTES {
        let error = open_verified(&good[..end], &registry).err();
        assert_eq!(
            error,
            Some(OpenError::Unreadable(Error::Truncated)),
            "{end}"
        );
        assert_eq!(error.and_then(|error| error.reason_key()), None);
    }
    let mut bad_magic = good.clone();
    bad_magic[0] ^= 1;
    assert_eq!(
        open_verified(&bad_magic, &registry).err(),
        Some(OpenError::Unreadable(Error::Magic))
    );
    // A current-version file with a damaged body is invalid, not an unknown format.
    let mut damaged = good;
    let last = damaged.len() - 1;
    damaged[last] ^= 1;
    assert_eq!(
        open_verified(&damaged, &registry).err(),
        Some(OpenError::Unreadable(Error::IntegrityFailed))
    );
    assert_eq!(header_format_version(&[]), Err(Error::Truncated));
}

#[test]
fn the_existing_single_version_check_maps_to_the_same_key() {
    assert_eq!(
        Error::UnsupportedVersion.reason_key(),
        Some("snapshot.format_unknown")
    );
    assert_eq!(Error::Magic.reason_key(), None);
}
