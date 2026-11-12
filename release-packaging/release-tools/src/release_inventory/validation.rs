use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    fmt,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

use super::*;

/// Validates release data, normalizes its ordering, and rehashes every declared staged file.
///
/// The staging root must contain package and executable files referenced by the input manifest.
/// Hashes describe the file handles read during this call; the staging tree is not locked and files
/// can change after this function returns. Callers must keep staging immutable and rehash final
/// published artifacts in the release gate. This function does not inspect package contents,
/// establish executable membership, validate signatures, create or verify build provenance, or
/// publish anything.
pub fn prepare_inventory(
    mut manifest: ReleaseManifest,
    staging_root: &Path,
) -> Result<(ReleaseInventory, Vec<VerifiedFile>), InventoryError> {
    validate_and_sort(&mut manifest)?;
    let verified = verify_staged_files(&manifest, staging_root)?;
    let inventory = ReleaseInventory {
        product_name: manifest.product_name,
        version: manifest.version,
        rows: manifest.rows,
        channel_references: manifest.channel_references,
    };
    Ok((inventory, verified))
}

/// Validates release metadata and sorts rows by format, OS family/version scope, and CPU.
///
/// Filename checks accept `x64`/`x86_64`/`amd64` and `arm64`/`aarch64` aliases used by the
/// published architecture and F15 requirements rather than imposing one platform's spelling.
pub fn validate_and_sort(manifest: &mut ReleaseManifest) -> Result<(), InventoryError> {
    require_nonempty("product_name", &manifest.product_name)?;
    require_nonempty("version", &manifest.version)?;
    if manifest.rows.is_empty() {
        return Err(InventoryError::InvalidField {
            field: "rows",
            detail: "must contain at least one distribution row".to_owned(),
        });
    }

    for row in &mut manifest.rows {
        validate_row(&manifest.product_name, &manifest.version, row)?;
        if let OsVersionScope::Exact { supported_versions } = &mut row.os.versions {
            supported_versions.sort();
        }
    }
    manifest.rows.sort_by(compare_rows);

    let mut names = std::collections::BTreeSet::new();
    for row in &manifest.rows {
        if !names.insert(row.file_name.to_ascii_lowercase()) {
            return Err(InventoryError::DuplicateArtifact(row.file_name.clone()));
        }
    }

    validate_channel_references(manifest)?;
    validate_staged_entries(manifest)?;
    manifest.channel_references.sort_by(|left, right| {
        left.channel
            .as_str()
            .cmp(right.channel.as_str())
            .then_with(|| left.version.cmp(&right.version))
            .then_with(|| left.cpu.sort_key().cmp(right.cpu.sort_key()))
            .then_with(|| left.artifact_file_name.cmp(&right.artifact_file_name))
    });
    Ok(())
}

fn validate_row(
    product_name: &str,
    release_version: &str,
    row: &DistributionRow,
) -> Result<(), InventoryError> {
    validate_artifact_name(&row.file_name)?;
    if !row
        .file_name
        .to_ascii_lowercase()
        .contains(&product_name.to_ascii_lowercase())
    {
        return Err(InventoryError::InvalidField {
            field: "file_name",
            detail: format!("{} must contain product name {product_name}", row.file_name),
        });
    }
    if !row
        .file_name
        .to_ascii_lowercase()
        .contains(&release_version.to_ascii_lowercase())
    {
        return Err(InventoryError::InvalidField {
            field: "file_name",
            detail: format!("{} must contain version {release_version}", row.file_name),
        });
    }
    if !row
        .cpu
        .filename_aliases()
        .iter()
        .any(|alias| contains_filename_token(&row.file_name, alias))
    {
        return Err(InventoryError::InvalidField {
            field: "file_name",
            detail: format!(
                "{} must identify CPU {} using a supported filename spelling",
                row.file_name,
                row.cpu.sort_key()
            ),
        });
    }
    if row.version != release_version {
        return Err(InventoryError::InvalidField {
            field: "version",
            detail: format!(
                "{} does not match release version {release_version}",
                row.version
            ),
        });
    }
    match &row.os.versions {
        OsVersionScope::Range { minimum, maximum } => {
            if maximum.as_ref().is_some_and(|maximum| maximum < minimum) {
                return Err(InventoryError::InvalidField {
                    field: "os",
                    detail: "maximum version must be greater than or equal to minimum".to_owned(),
                });
            }
        }
        OsVersionScope::Exact { supported_versions } => {
            if supported_versions.is_empty() {
                return Err(InventoryError::InvalidField {
                    field: "os",
                    detail: "exact supported versions must not be empty".to_owned(),
                });
            }
            let mut sorted_versions = supported_versions.clone();
            sorted_versions.sort();
            if sorted_versions.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(InventoryError::InvalidField {
                    field: "os",
                    detail: "exact supported versions must not contain duplicates".to_owned(),
                });
            }
        }
    }
    require_nonempty("signature_scope", &row.signature_scope)?;
    validate_artifact_name(&row.executable_file_name)?;
    require_nonempty("run_instructions", &row.run_instructions)?;
    Ok(())
}

fn validate_channel_references(manifest: &ReleaseManifest) -> Result<(), InventoryError> {
    let mut channel_keys = std::collections::BTreeSet::new();
    for reference in &manifest.channel_references {
        require_nonempty("channel_reference.version", &reference.version)?;
        if reference.version != manifest.version {
            return Err(InventoryError::InvalidChannelReference(format!(
                "{} reference version {} does not match release version {}",
                reference.channel.as_str(),
                reference.version,
                manifest.version
            )));
        }
        validate_artifact_name(&reference.artifact_file_name)?;
        let key = (
            reference.channel.as_str(),
            reference.version.as_str(),
            reference.cpu.sort_key(),
        );
        if !channel_keys.insert(key) {
            return Err(InventoryError::InvalidChannelReference(format!(
                "duplicate {} reference for version {} CPU {}",
                reference.channel.as_str(),
                reference.version,
                reference.cpu.sort_key()
            )));
        }
        let mut targets = manifest.rows.iter().filter(|row| {
            row.file_name == reference.artifact_file_name
                && row.version == reference.version
                && row.cpu == reference.cpu
        });
        if targets.next().is_none() || targets.next().is_some() {
            return Err(InventoryError::InvalidChannelReference(format!(
                "{} must reference exactly one row with version {} and CPU {}",
                reference.artifact_file_name,
                reference.version,
                reference.cpu.sort_key()
            )));
        }
    }
    Ok(())
}

fn validate_staged_entries(manifest: &ReleaseManifest) -> Result<(), InventoryError> {
    let expected = manifest.rows.len().saturating_mul(2);
    if manifest.staged_files.len() != expected {
        return Err(InventoryError::InvalidStagedFile(format!(
            "expected {expected} package/executable entries, got {}",
            manifest.staged_files.len()
        )));
    }
    let mut keys = std::collections::BTreeSet::new();
    for staged in &manifest.staged_files {
        if !manifest
            .rows
            .iter()
            .any(|row| row.file_name == staged.artifact_file_name)
        {
            return Err(InventoryError::InvalidStagedFile(format!(
                "unknown artifact {}",
                staged.artifact_file_name
            )));
        }
        if !keys.insert((staged.artifact_file_name.as_str(), staged.kind)) {
            return Err(InventoryError::InvalidStagedFile(format!(
                "duplicate {:?} entry for {}",
                staged.kind, staged.artifact_file_name
            )));
        }
        validate_relative_path(&staged.relative_path)?;
    }
    for row in &manifest.rows {
        for kind in [StagedFileKind::Package, StagedFileKind::Executable] {
            if !keys.contains(&(row.file_name.as_str(), kind)) {
                return Err(InventoryError::InvalidStagedFile(format!(
                    "missing {:?} entry for {}",
                    kind, row.file_name
                )));
            }
        }
    }
    Ok(())
}

fn verify_staged_files(
    manifest: &ReleaseManifest,
    staging_root: &Path,
) -> Result<Vec<VerifiedFile>, InventoryError> {
    let root = fs::canonicalize(staging_root).map_err(|source| InventoryError::Io {
        path: staging_root.to_path_buf(),
        source,
    })?;
    let mut verified = Vec::with_capacity(manifest.staged_files.len());
    for row in &manifest.rows {
        for kind in [StagedFileKind::Package, StagedFileKind::Executable] {
            let staged = manifest
                .staged_files
                .iter()
                .find(|item| item.artifact_file_name == row.file_name && item.kind == kind)
                .ok_or_else(|| {
                    InventoryError::InvalidStagedFile(format!(
                        "missing {:?} entry for {}",
                        kind, row.file_name
                    ))
                })?;
            if kind == StagedFileKind::Package
                && staged.relative_path.file_name() != Some(std::ffi::OsStr::new(&row.file_name))
            {
                return Err(InventoryError::InvalidStagedFile(format!(
                    "package path for {} must end in its artifact filename",
                    row.file_name
                )));
            }
            if kind == StagedFileKind::Executable
                && staged.relative_path.file_name()
                    != Some(std::ffi::OsStr::new(&row.executable_file_name))
            {
                return Err(InventoryError::InvalidStagedFile(format!(
                    "executable path for {} must end in {}",
                    row.file_name, row.executable_file_name
                )));
            }
            let (file, resolved) = open_staged_file(&root, &staged.relative_path)?;
            let actual = hash_file(file, &resolved)?;
            let expected = match kind {
                StagedFileKind::Package => &row.sha256,
                StagedFileKind::Executable => &row.executable_sha256,
            };
            if &actual != expected {
                return Err(InventoryError::DigestMismatch {
                    artifact_file_name: row.file_name.clone(),
                    kind,
                    expected: expected.clone(),
                    actual,
                });
            }
            verified.push(VerifiedFile {
                artifact_file_name: row.file_name.clone(),
                file_name: match kind {
                    StagedFileKind::Package => row.file_name.clone(),
                    StagedFileKind::Executable => row.executable_file_name.clone(),
                },
                kind,
                sha256: actual,
            });
        }
    }
    Ok(verified)
}

fn open_staged_file(root: &Path, relative: &Path) -> Result<(File, PathBuf), InventoryError> {
    let candidate = root.join(relative);
    let metadata = fs::symlink_metadata(&candidate).map_err(|source| InventoryError::Io {
        path: candidate.clone(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(InventoryError::InvalidStagedFile(format!(
            "{} must be a regular, non-symlink file",
            relative.display()
        )));
    }
    let resolved = fs::canonicalize(&candidate).map_err(|source| InventoryError::Io {
        path: candidate,
        source,
    })?;
    if !resolved.starts_with(root) {
        return Err(InventoryError::InvalidStagedFile(format!(
            "{} resolves outside the staging root",
            relative.display()
        )));
    }
    let file = File::open(&resolved).map_err(|source| InventoryError::Io {
        path: resolved.clone(),
        source,
    })?;
    let opened_metadata = file.metadata().map_err(|source| InventoryError::Io {
        path: resolved.clone(),
        source,
    })?;
    let current_path_metadata =
        fs::symlink_metadata(&resolved).map_err(|source| InventoryError::Io {
            path: resolved.clone(),
            source,
        })?;
    let current_path = fs::canonicalize(&resolved).map_err(|source| InventoryError::Io {
        path: resolved.clone(),
        source,
    })?;
    if !opened_metadata.is_file()
        || current_path_metadata.file_type().is_symlink()
        || !current_path_metadata.is_file()
        || current_path != resolved
        || !current_path.starts_with(root)
    {
        return Err(InventoryError::InvalidStagedFile(format!(
            "{} changed while it was being opened",
            relative.display()
        )));
    }
    Ok((file, resolved))
}

fn hash_file(mut file: File, path: &Path) -> Result<Sha256Digest, InventoryError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| InventoryError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").map_err(|_| InventoryError::InvalidField {
            field: "sha256",
            detail: "could not encode staged digest".to_owned(),
        })?;
    }
    Sha256Digest::parse(&encoded)
}

fn compare_rows(left: &DistributionRow, right: &DistributionRow) -> Ordering {
    left.format
        .sort_key()
        .cmp(right.format.sort_key())
        .then_with(|| left.os.family.sort_key().cmp(right.os.family.sort_key()))
        .then_with(|| left.os.versions.cmp(&right.os.versions))
        .then_with(|| left.cpu.sort_key().cmp(right.cpu.sort_key()))
        .then_with(|| left.file_name.cmp(&right.file_name))
}

fn validate_artifact_name(value: &str) -> Result<(), InventoryError> {
    require_nonempty("file_name", value)?;
    if value.contains('/') || value.contains('\\') || value == "." || value == ".." {
        return Err(InventoryError::InvalidField {
            field: "file_name",
            detail: "must be a single filename without path separators".to_owned(),
        });
    }
    Ok(())
}

fn contains_filename_token(file_name: &str, token: &str) -> bool {
    let name = file_name.to_ascii_lowercase();
    name.match_indices(token).any(|(start, matched)| {
        let end = start + matched.len();
        let before_is_boundary = start == 0
            || name[..start]
                .chars()
                .next_back()
                .is_some_and(|character| !character.is_ascii_alphanumeric());
        let after_is_boundary = end == name.len()
            || name[end..]
                .chars()
                .next()
                .is_some_and(|character| !character.is_ascii_alphanumeric());
        before_is_boundary && after_is_boundary
    })
}

fn validate_relative_path(path: &Path) -> Result<(), InventoryError> {
    if path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return Err(InventoryError::InvalidStagedFile(format!(
            "{} must be a nonempty relative path without parent traversal",
            path.display()
        )));
    }
    Ok(())
}

fn require_nonempty(field: &'static str, value: &str) -> Result<(), InventoryError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(InventoryError::InvalidField {
            field,
            detail: "must be nonempty and contain no control characters".to_owned(),
        });
    }
    Ok(())
}
