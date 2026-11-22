//! Conversion from a verified DUSNAP v1 stream into an isolated result version.

mod frames;
mod metadata;

use crate::{
    VerifiedSnapshot,
    records::{self, NameEncoding},
    zstd_stream::StagingSink,
};
use du_analysis_index::{
    BuildError, FileIdentity, Observation, SharingEvidence, SpaceObservation,
    snapshot_load::SnapshotBuild,
};
use du_core_contracts::{
    ByteSize, Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    Timestamp,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use metadata::{timestamp, volume_reports};
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
};

const VOLUME_KIND: u8 = 1;
const ITEM_KIND: u8 = 2;
const GAP_KIND: u8 = 3;
const END_KIND: u8 = 0x7f;

/// Failure while turning a verified snapshot into an immutable result version.
#[derive(Debug)]
pub enum LoadError {
    /// Loading was cancelled before the result was sealed.
    Cancelled,
    /// A verified record or metadata value could not be represented by the index.
    InvalidSnapshot,
    /// Reading the verified staged stream failed.
    Io(io::Error),
    /// The isolated index build rejected the source data.
    Build(BuildError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("snapshot load cancelled"),
            Self::InvalidSnapshot => {
                formatter.write_str("verified snapshot cannot be represented as a result")
            }
            Self::Io(_) => formatter.write_str("verified snapshot record read failed"),
            Self::Build(error) => write!(formatter, "snapshot result build failed: {error:?}"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Build(_) => None,
            Self::Cancelled | Self::InvalidSnapshot => None,
        }
    }
}

impl From<io::Error> for LoadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<BuildError> for LoadError {
    fn from(error: BuildError) -> Self {
        Self::Build(error)
    }
}

/// Loads a verified snapshot into a private read-only result version.
///
/// The result has `DataSource::Snapshot`, its snapshot ID, and no live analysis
/// lineage. Cancellation or any conversion error drops the isolated build before
/// returning. The operation never receives or changes a live result store. When
/// `scopeVersionId` is null, the source result-version ID is used because the
/// index requires a scope-version ID. An unknown completeness value is rejected
/// because the index has no unknown-completeness state.
pub fn load<W>(
    mut snapshot: VerifiedSnapshot<W>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Arc<du_analysis_index::ResultVersion>, LoadError>
where
    W: StagingSink + Read + Seek,
{
    if cancelled() {
        return Err(LoadError::Cancelled);
    }
    let metadata = snapshot.metadata();
    let snapshot_id = parse_id(metadata.get("snapshotId"))?;
    let source = metadata.get("source").ok_or(LoadError::InvalidSnapshot)?;
    let version_id = parse_id(source.get("resultVersionId"))?;
    let scope_version_id = match source.get("scopeVersionId") {
        Some(Value::String(_)) => parse_id(source.get("scopeVersionId"))?,
        Some(Value::Null) => version_id.clone(),
        _ => return Err(LoadError::InvalidSnapshot),
    };
    let observed_at = parse_time(source.get("observedAt"))?;
    let completeness = match snapshot.completeness() {
        Some("complete") => ResultCompleteness::Complete,
        Some("partial") => ResultCompleteness::Partial,
        Some("missing") => ResultCompleteness::Missing,
        None => return Err(LoadError::InvalidSnapshot),
        _ => return Err(LoadError::InvalidSnapshot),
    };
    let scope_fingerprint = source
        .get("scopeMarker")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let encoding = match metadata.get("pathEncoding").and_then(Value::as_str) {
        Some("bytes") => NameEncoding::Bytes,
        Some("utf16le") => NameEncoding::WindowsUtf16Le,
        _ => return Err(LoadError::InvalidSnapshot),
    };
    let (root_links, volume_ids, reports) = {
        let volume = snapshot
            .volume_record()
            .ok_or(LoadError::InvalidSnapshot)?
            .decode()
            .map_err(|_| LoadError::InvalidSnapshot)?;
        (
            volume.roots.clone(),
            volume
                .volumes
                .iter()
                .map(|entry| entry.id.to_vec())
                .collect::<Vec<_>>(),
            volume_reports(&volume.volumes)?,
        )
    };
    let root_by_item = root_item_map(&root_links)?;
    let root_file_ids =
        read_root_identities(&mut snapshot, &root_by_item, encoding, &mut cancelled)?;
    let roots = build_roots(&snapshot, &root_links, &volume_ids, root_file_ids, encoding)?;
    let mut build = SnapshotBuild::open(
        snapshot_id,
        version_id,
        scope_version_id,
        roots,
        scope_fingerprint,
        observed_at,
        completeness,
    )?;
    build.build_mut().set_volume_reports(reports)?;
    load_records(
        &mut snapshot,
        &mut build,
        &root_by_item,
        encoding,
        &mut cancelled,
    )?;
    if cancelled() {
        return Err(LoadError::Cancelled);
    }
    build.seal().map_err(Into::into)
}

fn parse_id(value: Option<&Value>) -> Result<Id, LoadError> {
    value
        .and_then(Value::as_str)
        .and_then(|value| Id::parse(value).ok())
        .ok_or(LoadError::InvalidSnapshot)
}

fn parse_time(value: Option<&Value>) -> Result<Option<Timestamp>, LoadError> {
    match value {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Timestamp::parse(value)
            .map(Some)
            .map_err(|_| LoadError::InvalidSnapshot),
        _ => Err(LoadError::InvalidSnapshot),
    }
}

fn root_item_map(roots: &[records::volume::RootLink]) -> Result<HashMap<u32, u32>, LoadError> {
    let mut mapping = HashMap::with_capacity(roots.len());
    for (root_index, root) in roots.iter().enumerate() {
        let item_number = root.item_number.ok_or(LoadError::InvalidSnapshot)?;
        let root_index = u32::try_from(root_index).map_err(|_| LoadError::InvalidSnapshot)?;
        if mapping.insert(item_number, root_index).is_some() {
            return Err(LoadError::InvalidSnapshot);
        }
    }
    Ok(mapping)
}

fn read_root_identities<W>(
    snapshot: &mut VerifiedSnapshot<W>,
    root_by_item: &HashMap<u32, u32>,
    encoding: NameEncoding,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<Vec<u8>>, LoadError>
where
    W: StagingSink + Read + Seek,
{
    let mut identities = vec![None; root_by_item.len()];
    let mut reader = snapshot.records();
    reader.seek(SeekFrom::Start(0))?;
    let mut first = true;
    while let Some((kind, body)) = frames::read_record(&mut reader, cancelled)? {
        match kind {
            VOLUME_KIND if first => {}
            ITEM_KIND => {
                let item = records::item::decode(&body, encoding)
                    .map_err(|_| LoadError::InvalidSnapshot)?;
                if item.parent == 0
                    && let Some(index) = root_by_item.get(&item.number)
                {
                    identities[*index as usize] = Some(item.identity.to_vec());
                }
            }
            GAP_KIND => {}
            END_KIND => break,
            _ => return Err(LoadError::InvalidSnapshot),
        }
        first = false;
    }
    identities
        .into_iter()
        .map(|identity| identity.ok_or(LoadError::InvalidSnapshot))
        .collect()
}

fn build_roots<W>(
    snapshot: &VerifiedSnapshot<W>,
    links: &[records::volume::RootLink],
    volume_ids: &[Vec<u8>],
    file_ids: Vec<Vec<u8>>,
    encoding: NameEncoding,
) -> Result<Vec<ResultScopeRoot>, LoadError>
where
    W: StagingSink + Read + Seek,
{
    if snapshot.scope_roots().len() != links.len() || file_ids.len() != links.len() {
        return Err(LoadError::InvalidSnapshot);
    }
    let path_encoding = match encoding {
        NameEncoding::Bytes => PathEncoding::Unix,
        NameEncoding::WindowsUtf16Le => PathEncoding::WindowsUtf16le,
    };
    snapshot
        .scope_roots()
        .iter()
        .zip(links)
        .zip(file_ids)
        .map(|((path, link), file_id)| {
            let volume_id = link
                .volume_index
                .map(|index| {
                    volume_ids
                        .get(usize::from(index))
                        .cloned()
                        .ok_or(LoadError::InvalidSnapshot)
                })
                .transpose()?
                .unwrap_or_default();
            Ok(ResultScopeRoot {
                path: ResultNativePath {
                    encoding: path_encoding.clone(),
                    raw: PathBytes::from_bytes(path),
                },
                volume_id,
                file_id,
            })
        })
        .collect()
}

fn load_records<W>(
    snapshot: &mut VerifiedSnapshot<W>,
    build: &mut SnapshotBuild,
    root_by_item: &HashMap<u32, u32>,
    encoding: NameEncoding,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), LoadError>
where
    W: StagingSink + Read + Seek,
{
    let mut reader = snapshot.records();
    reader.seek(SeekFrom::Start(0))?;
    let mut first = true;
    let mut names = Vec::<Vec<u8>>::new();
    let mut parents = Vec::<Option<u32>>::new();
    let mut item_roots = Vec::<u32>::new();
    while let Some((kind, body)) = frames::read_record(&mut reader, cancelled)? {
        match kind {
            VOLUME_KIND if first => {}
            ITEM_KIND => {
                let item = records::item::decode(&body, encoding)
                    .map_err(|_| LoadError::InvalidSnapshot)?;
                let index = usize::try_from(
                    item.number
                        .checked_sub(1)
                        .ok_or(LoadError::InvalidSnapshot)?,
                )
                .map_err(|_| LoadError::InvalidSnapshot)?;
                if index != names.len() {
                    return Err(LoadError::InvalidSnapshot);
                }
                let (root_index, path) = if item.parent == 0 {
                    (
                        *root_by_item
                            .get(&item.number)
                            .ok_or(LoadError::InvalidSnapshot)?,
                        Vec::new(),
                    )
                } else {
                    let parent_index =
                        usize::try_from(item.parent - 1).map_err(|_| LoadError::InvalidSnapshot)?;
                    let root_index = *item_roots
                        .get(parent_index)
                        .ok_or(LoadError::InvalidSnapshot)?;
                    (
                        root_index,
                        item_path(item.parent, item.name, &parents, &names)?,
                    )
                };
                add_item(build, root_index, path.clone(), &item)?;
                if let Some(modified_ms) = item.modified_ms {
                    let time = Timestamp::from_unix_millis(modified_ms)
                        .ok_or(LoadError::InvalidSnapshot)?;
                    build.build_mut().set_modified_at(root_index, &path, time)?;
                }
                names.push(item.name.to_vec());
                parents.push((item.parent != 0).then_some(item.parent));
                item_roots.push(root_index);
            }
            GAP_KIND => {
                let gap = records::gap::decode(&body, encoding, root_by_item.len())
                    .map_err(|_| LoadError::InvalidSnapshot)?;
                build.build_mut().record_gap_at(
                    gap.root_index,
                    gap.path.into_iter().map(<[u8]>::to_vec).collect(),
                    gap.reason_key.to_owned(),
                    timestamp(gap.observed_at_ms)?,
                )?;
            }
            END_KIND => break,
            _ => return Err(LoadError::InvalidSnapshot),
        }
        first = false;
    }
    Ok(())
}

fn item_path(
    parent_number: u32,
    name: &[u8],
    parents: &[Option<u32>],
    names: &[Vec<u8>],
) -> Result<Vec<Vec<u8>>, LoadError> {
    let mut reversed = vec![name.to_vec()];
    let mut current = parent_number;
    while current != 0 {
        let index = usize::try_from(current - 1).map_err(|_| LoadError::InvalidSnapshot)?;
        let parent = *parents.get(index).ok_or(LoadError::InvalidSnapshot)?;
        if let Some(parent) = parent {
            reversed.push(names[index].clone());
            current = parent;
        } else {
            break;
        }
    }
    reversed.reverse();
    Ok(reversed)
}

fn add_item(
    build: &mut SnapshotBuild,
    root_index: u32,
    path: Vec<Vec<u8>>,
    item: &records::item::Item<'_>,
) -> Result<(), LoadError> {
    let build = build.build_mut();
    let logical = size_value(item.sizes[0], SizeBasis::Logical);
    match item.kind {
        records::item::Kind::Folder => build.add_directory_at(root_index, path)?,
        records::item::Kind::File | records::item::Kind::Link => {
            let root_volume = build
                .root_volumes()
                .get(root_index as usize)
                .cloned()
                .ok_or(LoadError::InvalidSnapshot)?;
            let identity =
                (!item.identity.is_empty() && !root_volume.is_empty()).then(|| FileIdentity {
                    volume: root_volume,
                    file: item.identity.to_vec(),
                });
            let space = SpaceObservation {
                allocated: Some(size_value(item.sizes[1], SizeBasis::Allocated)),
                identity,
                link_count: (item.link_count != 0).then_some(u64::from(item.link_count)),
                sharing: SharingEvidence::Unknown,
            };
            let observation = Observation {
                path,
                size: Some(logical),
            };
            if item.kind == records::item::Kind::File {
                build.add_with_space_at(root_index, observation, space)?;
            } else {
                build.add_observed_untraversed_link_at(root_index, observation, Some(space))?;
            }
        }
        records::item::Kind::Other => build.add_at(root_index, Observation { path, size: None })?,
    }
    Ok(())
}

fn size_value(value: records::item::Size, basis: SizeBasis) -> SizeValue {
    let (amount, evidence) = match value {
        records::item::Size::Exact(bytes) => (
            SizeAmount::Exact {
                bytes: ByteSize::from(bytes),
            },
            SizeEvidence::OsReported,
        ),
        records::item::Size::LowerBound(bytes) => (
            SizeAmount::LowerBound {
                bytes: ByteSize::from(bytes),
            },
            SizeEvidence::OsReported,
        ),
        records::item::Size::Unknown => (SizeAmount::Unknown, SizeEvidence::Unknown),
    };
    SizeValue {
        basis,
        evidence,
        amount,
    }
}
