use super::{Error, PathSeparator, SnapshotIdentity, SourceMetadata};
use crate::records::{NameEncoding, item, volume};
use du_analysis_index::{ResultVersion, VolumeReport, VolumeReportPoint};
use du_core_contracts::{
    PathEncoding, SizeAmount, Timestamp,
    ports::PortErrorKind,
    result::{ResultItem, ResultItemKind, ResultScopeRoot, ResultVersionRead},
    scan_observation::VolumeSpace,
};
use std::collections::{HashMap, HashSet};

pub(super) fn path_encoding(roots: &[ResultScopeRoot]) -> Result<NameEncoding, Error> {
    let first = roots.first().ok_or(Error::InvalidResult)?;
    let encoding = match first.path.encoding {
        PathEncoding::Unix => NameEncoding::Bytes,
        PathEncoding::WindowsUtf16le => NameEncoding::WindowsUtf16Le,
    };
    if roots.iter().any(|root| {
        !matches!(
            (&root.path.encoding, encoding),
            (PathEncoding::Unix, NameEncoding::Bytes)
                | (PathEncoding::WindowsUtf16le, NameEncoding::WindowsUtf16Le)
        )
    }) {
        return Err(Error::InvalidResult);
    }
    Ok(encoding)
}

pub(super) fn validate_source(
    identity: &SnapshotIdentity<'_>,
    source: &SourceMetadata,
    roots: &[ResultScopeRoot],
    encoding: NameEncoding,
) -> Result<(), Error> {
    if identity.app_version.is_empty() || source.path_rules.len() != roots.len() {
        return Err(Error::InvalidMetadata);
    }
    let expected = match encoding {
        NameEncoding::Bytes => PathSeparator::Slash,
        NameEncoding::WindowsUtf16Le => PathSeparator::Backslash,
    };
    if source
        .path_rules
        .iter()
        .any(|rule| rule.separator != expected)
    {
        return Err(Error::InvalidMetadata);
    }
    let mut algorithms = HashSet::new();
    for algorithm in &source.algorithms {
        if algorithm.kind.is_empty()
            || algorithm.id.is_empty()
            || algorithm.version == 0
            || !algorithms.insert((&algorithm.kind, &algorithm.id, algorithm.version))
        {
            return Err(Error::InvalidMetadata);
        }
    }
    Ok(())
}

pub(super) fn root_items(items: &[ResultItem], root_count: usize) -> Result<Vec<u32>, Error> {
    let mut root_numbers = Vec::with_capacity(root_count);
    let mut root_for_item = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        if usize::try_from(item.item_no).ok() != Some(index) {
            return Err(Error::InvalidResult);
        }
        let root = match item.parent {
            None => {
                let root = u32::try_from(root_numbers.len()).map_err(|_| Error::InvalidResult)?;
                root_numbers.push(item.item_no.checked_add(1).ok_or(Error::InvalidResult)?);
                root
            }
            Some(parent) => {
                let parent_index = usize::try_from(parent).map_err(|_| Error::InvalidResult)?;
                if parent_index >= index {
                    return Err(Error::InvalidResult);
                }
                *root_for_item
                    .get(parent_index)
                    .ok_or(Error::InvalidResult)?
            }
        };
        root_for_item.push(root);
    }
    if root_numbers.len() != root_count
        || root_numbers
            .iter()
            .enumerate()
            .any(|(index, number)| usize::try_from(*number).ok() != Some(index + 1))
    {
        return Err(Error::InvalidResult);
    }
    Ok(root_numbers)
}

pub(super) fn volume_records<'a>(
    version: &'a ResultVersion,
    roots: &'a [ResultScopeRoot],
    root_items: &[u32],
) -> Result<volume::Volume<'a>, Error> {
    if roots.len() != root_items.len() {
        return Err(Error::InvalidResult);
    }
    let mut entries = Vec::new();
    let mut indices = HashMap::<Vec<u8>, u16>::new();
    for report in version.volume_reports() {
        if report.volume_id.is_empty() {
            entries.push(volume_entry(&report.volume_id, Some(report)));
        } else {
            if indices.contains_key(&report.volume_id) || entries.len() >= usize::from(u16::MAX) {
                return Err(Error::InvalidResult);
            }
            let index = u16::try_from(entries.len()).map_err(|_| Error::InvalidRecord)?;
            entries.push(volume_entry(&report.volume_id, Some(report)));
            indices.insert(report.volume_id.clone(), index);
        }
    }

    for root in roots.iter().filter(|root| !root.volume_id.is_empty()) {
        if !indices.contains_key(&root.volume_id) {
            if entries.len() >= usize::from(u16::MAX) {
                return Err(Error::InvalidRecord);
            }
            let index = u16::try_from(entries.len()).map_err(|_| Error::InvalidRecord)?;
            entries.push(volume_entry(&root.volume_id, None));
            indices.insert(root.volume_id.clone(), index);
        }
    }
    if entries.len() > usize::from(u16::MAX) {
        return Err(Error::InvalidRecord);
    }
    let links = roots
        .iter()
        .zip(root_items)
        .map(|(root, item_number)| volume::RootLink {
            volume_index: (!root.volume_id.is_empty())
                .then(|| indices.get(&root.volume_id).copied())
                .flatten(),
            item_number: Some(*item_number),
        })
        .collect();
    Ok(volume::Volume {
        roots: links,
        volumes: entries,
    })
}

fn volume_entry<'a>(id: &'a [u8], report: Option<&'a VolumeReport>) -> volume::VolumeEntry<'a> {
    let (start, end) = match report {
        Some(report) => (
            volume_observation(report.start.as_ref()),
            volume_observation(report.end.as_ref()),
        ),
        None => (
            volume::Observation::NotAttempted,
            volume::Observation::NotAttempted,
        ),
    };
    volume::VolumeEntry {
        id,
        basis: "",
        start,
        end,
    }
}

fn volume_observation(point: Option<&VolumeReportPoint>) -> volume::Observation<'_> {
    match point {
        None => volume::Observation::NotAttempted,
        Some(point) => {
            let observed_ms = point.observed_at.as_ref().map(Timestamp::unix_millis);
            match &point.value {
                Ok(VolumeSpace {
                    total_bytes,
                    used_bytes,
                    free_bytes,
                }) => volume::Observation::Success {
                    observed_ms,
                    total: *total_bytes,
                    used: *used_bytes,
                    free: *free_bytes,
                },
                Err(error) => volume::Observation::Failure {
                    observed_ms,
                    kind: port_error_kind(&error.kind),
                    reason: &error.reason_key,
                    os_code: error.os_code,
                },
            }
        }
    }
}

fn port_error_kind(kind: &PortErrorKind) -> &'static str {
    match kind {
        PortErrorKind::NotFound => "not_found",
        PortErrorKind::PermissionDenied => "permission_denied",
        PortErrorKind::InUse => "in_use",
        PortErrorKind::Unsupported => "unsupported",
        PortErrorKind::IdentityChanged => "identity_changed",
        PortErrorKind::Busy => "busy",
        PortErrorKind::Io => "io",
        PortErrorKind::Corrupt => "corrupt",
        PortErrorKind::SchemaMismatch => "schema_mismatch",
    }
}

pub(super) fn item_record<'a>(
    version: &'a ResultVersion,
    source: &'a ResultItem,
    roots: &'a [ResultScopeRoot],
) -> Result<item::Item<'a>, Error> {
    let root_index = root_index_for(version.items(), source.item_no)?;
    let root = roots.get(root_index).ok_or(Error::InvalidResult)?;
    let number = source.item_no.checked_add(1).ok_or(Error::InvalidResult)?;
    let parent = match source.parent {
        Some(parent) => parent.checked_add(1).ok_or(Error::InvalidResult)?,
        None => 0,
    };
    let kind = match source.kind {
        ResultItemKind::File => item::Kind::File,
        ResultItemKind::Directory => item::Kind::Folder,
        ResultItemKind::UntraversedLink => item::Kind::Link,
        ResultItemKind::Unknown | ResultItemKind::UnknownStructure => item::Kind::Other,
    };
    let space = version.space(source.item_no).ok_or(Error::InvalidResult)?;
    let identity = if source.parent.is_none() {
        root.file_id.as_slice()
    } else {
        version
            .file_identity_bytes(source.item_no)
            .filter(|(volume, _)| !root.volume_id.is_empty() && *volume == root.volume_id)
            .map_or(&[][..], |(_, file)| file)
    };
    let link_count = match version.file_link_count(source.item_no) {
        Some(count) => u32::try_from(count).map_err(|_| Error::InvalidResult)?,
        None => 0,
    };
    let name = if source.parent.is_none() {
        // The v1 validator defines `@` as the name of a scope-root Item.
        match root.path.encoding {
            PathEncoding::Unix => b"@".as_slice(),
            PathEncoding::WindowsUtf16le => &[b'@', 0],
        }
    } else {
        source.name.as_slice()
    };
    Ok(item::Item {
        number,
        parent,
        kind,
        flags: 0,
        name,
        sizes: [
            snapshot_size(&space.logical),
            snapshot_size(&space.allocated),
            snapshot_size(&space.unique_physical),
        ],
        modified_ms: version
            .item_modified_at(source.item_no)
            .map(Timestamp::unix_millis),
        identity,
        link_count,
    })
}

fn root_index_for(items: &[ResultItem], item_no: u32) -> Result<usize, Error> {
    let index = usize::try_from(item_no).map_err(|_| Error::InvalidResult)?;
    let mut current = items.get(index).ok_or(Error::InvalidResult)?;
    loop {
        match current.parent {
            None => return usize::try_from(current.item_no).map_err(|_| Error::InvalidResult),
            Some(parent) => {
                current = items
                    .get(usize::try_from(parent).map_err(|_| Error::InvalidResult)?)
                    .ok_or(Error::InvalidResult)?;
            }
        }
    }
}

fn snapshot_size(value: &du_core_contracts::SizeValue) -> item::Size {
    match &value.amount {
        SizeAmount::Exact { bytes } => item::Size::Exact(bytes.get()),
        SizeAmount::LowerBound { bytes } => item::Size::LowerBound(bytes.get()),
        SizeAmount::Unknown => item::Size::Unknown,
    }
}
