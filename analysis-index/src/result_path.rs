//! Resolve native item paths without accessing the filesystem or display text.
use du_core_contracts::{
    ItemRef, PathBytes, PathEncoding,
    result::{ResultNativePath, ResultVersionRead},
};

use crate::{QueryError, ResultVersion};

impl ResultVersion {
    /// Builds the observed absolute path from verified root bytes and stored names.
    ///
    /// A result without retained root evidence cannot supply an absolute path.
    /// The bytes remain in their original encoding and can contain names that
    /// cannot be displayed losslessly. This is not a current identity check.
    pub fn native_path(&self, item: &ItemRef) -> Result<ResultNativePath, QueryError> {
        let detail = self.detail(item)?;
        let root_index = *self
            .root_indices
            .get(detail.item.item_no as usize)
            .ok_or(QueryError::PathUnavailable)? as usize;
        let root = self
            .roots
            .get(root_index)
            .or_else(|| self.observed_root())
            .ok_or(QueryError::PathUnavailable)?;
        let mut raw = root.path.raw.bytes().map_err(|_| QueryError::InvalidPath)?;
        let separator = match root.path.encoding {
            PathEncoding::Unix if raw.starts_with(b"/") && !raw.contains(&0) => b"/".as_slice(),
            PathEncoding::WindowsUtf16le if valid_windows_root(&raw) => b"\\\0".as_slice(),
            _ => return Err(QueryError::InvalidPath),
        };
        for part in self.ancestors(detail.item)?.into_iter().skip(1) {
            if !valid_name(&root.path.encoding, &part.name) {
                return Err(QueryError::InvalidPath);
            }
            if !raw.ends_with(separator) {
                raw.extend_from_slice(separator);
            }
            raw.extend_from_slice(&part.name);
        }
        Ok(ResultNativePath {
            encoding: root.path.encoding.clone(),
            raw: PathBytes::from_bytes(&raw),
        })
    }
}

fn valid_windows_root(raw: &[u8]) -> bool {
    let drive = raw.strip_prefix(b"\\\0\\\0?\0\\\0").unwrap_or(raw);
    let absolute =
        drive.first().is_some_and(u8::is_ascii_alphabetic) && drive.get(1..6) == Some(b"\0:\0\\\0");
    let (units, tail) = raw.as_chunks::<2>();
    absolute && tail.is_empty() && !units.contains(&[0, 0])
}

fn valid_name(encoding: &PathEncoding, name: &[u8]) -> bool {
    if name.is_empty() {
        return false;
    }
    match encoding {
        PathEncoding::Unix => {
            !matches!(name, b"." | b"..") && !name.iter().any(|b| matches!(b, 0 | b'/'))
        }
        PathEncoding::WindowsUtf16le => {
            let (units, tail) = name.as_chunks::<2>();
            tail.is_empty()
                && !matches!(name, b".\0" | b".\0.\0")
                && !units
                    .iter()
                    .any(|unit| matches!(u16::from_le_bytes(*unit), 0 | 47 | 58 | 92))
        }
    }
}
