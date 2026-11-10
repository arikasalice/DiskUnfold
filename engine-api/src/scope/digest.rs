//! Canonical scope and review fingerprints for F01 scope admission.

use du_core_contracts::{
    OsPath, PathEncoding,
    analysis_ipc::{ScopeBoundary, ScopeDraft, ScopeExclusion},
};

use crate::error::EngineError;

/// One original selection and the identity confirmed by opening it.
pub(super) struct ReviewRoot<'a> {
    pub path: &'a OsPath,
    pub identity: Option<(&'a [u8], &'a [u8])>,
}

type PathKey = (u8, Vec<u8>);

fn path_key(path: &OsPath) -> Result<PathKey, EngineError> {
    let encoding = match path.encoding {
        PathEncoding::Unix => 0,
        PathEncoding::WindowsUtf16le => 1,
    };
    let bytes = path
        .raw
        .bytes()
        .map_err(|_| EngineError::reason("path.unresolvable"))?;
    Ok((encoding, bytes))
}

fn length(hash: &mut blake3::Hasher, len: usize) {
    hash.update(&(len as u64).to_le_bytes());
}

fn bytes(hash: &mut blake3::Hasher, value: &[u8]) {
    length(hash, value.len());
    hash.update(value);
}

fn path(hash: &mut blake3::Hasher, (encoding, raw): &PathKey) {
    hash.update(&[*encoding]);
    bytes(hash, raw);
}

fn paths(hash: &mut blake3::Hasher, values: &[OsPath]) -> Result<(), EngineError> {
    let mut keys = values.iter().map(path_key).collect::<Result<Vec<_>, _>>()?;
    keys.sort_unstable();
    length(hash, keys.len());
    for key in &keys {
        path(hash, key);
    }
    Ok(())
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum ExclusionKey<'a> {
    Path(PathKey),
    NamePattern(&'a [u8]),
}

fn exclusions(hash: &mut blake3::Hasher, rules: &[ScopeExclusion]) -> Result<(), EngineError> {
    let mut keys = rules
        .iter()
        .map(|rule| match rule {
            ScopeExclusion::Path { path } => path_key(path).map(ExclusionKey::Path),
            ScopeExclusion::NamePattern { pattern } => {
                Ok(ExclusionKey::NamePattern(pattern.as_bytes()))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    keys.sort_unstable();
    length(hash, keys.len());
    for key in keys {
        match key {
            ExclusionKey::Path(key) => {
                hash.update(&[0]);
                path(hash, &key);
            }
            ExclusionKey::NamePattern(key) => {
                hash.update(&[1]);
                bytes(hash, key);
            }
        }
    }
    Ok(())
}

/// Hashes normalized path keys and scope conditions independently of their observation.
pub(super) fn scope(draft: &ScopeDraft) -> Result<blake3::Hash, EngineError> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"diskunfold.scope.v1\0");
    let boundary = match draft.boundary {
        ScopeBoundary::CurrentVolume => 0,
        ScopeBoundary::IncludeSelected => 1,
    };
    hash.update(&[boundary]);
    paths(&mut hash, &draft.includes)?;
    exclusions(&mut hash, &draft.exclusions)?;
    paths(&mut hash, &draft.included_mounts)?;
    Ok(hash.finalize())
}

/// Binds every selected root's observed identity to the scope reviewed by the caller.
pub(super) fn review(
    scope: &blake3::Hash,
    roots: &[ReviewRoot<'_>],
) -> Result<blake3::Hash, EngineError> {
    let mut selected = roots
        .iter()
        .map(|root| Ok((path_key(root.path)?, root.identity)))
        .collect::<Result<Vec<_>, EngineError>>()?;
    selected.sort_unstable();
    let mut hash = blake3::Hasher::new();
    hash.update(b"diskunfold.scope-review.v1\0");
    hash.update(scope.as_bytes());
    length(&mut hash, selected.len());
    for (key, identity) in selected {
        path(&mut hash, &key);
        hash.update(&[u8::from(identity.is_some())]);
        let (volume, file) = identity.unwrap_or_default();
        bytes(&mut hash, volume);
        bytes(&mut hash, file);
    }
    Ok(hash.finalize())
}

#[cfg(test)]
mod tests;
