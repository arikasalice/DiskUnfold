//! DUSNAP v1 Volume body codec. Native identifiers and unknown keys are preserved.
use du_core_contracts::Timestamp;
use std::collections::HashSet;

/// Mapping for one scope root, in metadata order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootLink {
    /// Index of a confirmed volume, or none if unknown.
    pub volume_index: Option<u16>,
    /// Number of the corresponding top-level folder, or none if unknown.
    pub item_number: Option<u32>,
}
/// A volume capacity observation, preserving unknown failure keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Observation<'a> {
    /// No observation was attempted.
    NotAttempted,
    /// An observation with raw byte counts.
    Success {
        /// Observed Unix milliseconds, or none when unknown.
        observed_ms: Option<i64>,
        /// Total bytes.
        total: u64,
        /// Used bytes.
        used: u64,
        /// Free bytes.
        free: u64,
    },
    /// An observation that failed with a reason and optional OS code.
    Failure {
        /// Observed Unix milliseconds, or none when unknown.
        observed_ms: Option<i64>,
        /// Failure kind, including unknown keys.
        kind: &'a str,
        /// Failure reason key.
        reason: &'a str,
        /// Optional native error code.
        os_code: Option<i32>,
    },
}
/// One observed volume; empty identifiers and bases mean unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeEntry<'a> {
    /// Native identifier bytes.
    pub id: &'a [u8],
    /// Original reporting basis.
    pub basis: &'a str,
    /// Observation at the start.
    pub start: Observation<'a>,
    /// Observation at the end.
    pub end: Observation<'a>,
}
/// Root mappings and ordered volume observations from one v1 body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume<'a> {
    /// Mappings in source scope-root order.
    pub roots: Vec<RootLink>,
    /// Volume observations in capture order.
    pub volumes: Vec<VolumeEntry<'a>>,
}
/// A malformed, oversized, or unrepresentable Volume body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error;

fn take<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8], Error> {
    if input.len() < n {
        return Err(Error);
    }
    let (a, b) = input.split_at(n);
    *input = b;
    Ok(a)
}
fn array<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], Error> {
    take(input, N)?.try_into().map_err(|_| Error)
}
fn blob<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], Error> {
    let n = u16::from_le_bytes(array(input)?) as usize;
    take(input, n)
}
fn string<'a>(input: &mut &'a [u8]) -> Result<&'a str, Error> {
    std::str::from_utf8(blob(input)?).map_err(|_| Error)
}
fn key(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
fn time(input: &mut &[u8]) -> Result<Option<i64>, Error> {
    let t = i64::from_le_bytes(array(input)?);
    if t == i64::MIN {
        Ok(None)
    } else if Timestamp::from_unix_millis(t).is_some() {
        Ok(Some(t))
    } else {
        Err(Error)
    }
}
fn observation<'a>(input: &mut &'a [u8]) -> Result<Observation<'a>, Error> {
    match array::<1>(input)?[0] {
        0 => Ok(Observation::NotAttempted),
        1 => Ok(Observation::Success {
            observed_ms: time(input)?,
            total: u64::from_le_bytes(array(input)?),
            used: u64::from_le_bytes(array(input)?),
            free: u64::from_le_bytes(array(input)?),
        }),
        2 => {
            let observed_ms = time(input)?;
            let kind = string(input)?;
            let reason = string(input)?;
            if !key(kind) || !key(reason) {
                return Err(Error);
            }
            let os_code = match array::<1>(input)?[0] {
                0 => None,
                1 => Some(i32::from_le_bytes(array(input)?)),
                _ => return Err(Error),
            };
            Ok(Observation::Failure {
                observed_ms,
                kind,
                reason,
                os_code,
            })
        }
        _ => Err(Error),
    }
}
/// Decode a complete bounded body against the root count from the same metadata.
pub fn decode(mut input: &[u8], root_count: usize) -> Result<Volume<'_>, Error> {
    if input.len() + 1 > 65_536 || array::<1>(&mut input)?[0] != 1 {
        return Err(Error);
    }
    let roots = u16::from_le_bytes(array(&mut input)?) as usize;
    let count = u16::from_le_bytes(array(&mut input)?) as usize;
    if roots != root_count || roots.checked_mul(6).ok_or(Error)? > input.len() {
        return Err(Error);
    }
    let mut links = Vec::with_capacity(roots);
    let mut items = HashSet::new();
    for _ in 0..roots {
        let v = u16::from_le_bytes(array(&mut input)?);
        let n = u32::from_le_bytes(array(&mut input)?);
        if v != u16::MAX && v as usize >= count || n != 0 && !items.insert(n) {
            return Err(Error);
        }
        links.push(RootLink {
            volume_index: (v != u16::MAX).then_some(v),
            item_number: (n != 0).then_some(n),
        });
    }
    if count > input.len() / 6 {
        return Err(Error);
    }
    let mut volumes = Vec::with_capacity(count);
    let mut ids = HashSet::new();
    for _ in 0..count {
        let id = blob(&mut input)?;
        if !id.is_empty() && !ids.insert(id) {
            return Err(Error);
        }
        let basis = string(&mut input)?;
        let start = observation(&mut input)?;
        let end = observation(&mut input)?;
        volumes.push(VolumeEntry {
            id,
            basis,
            start,
            end,
        });
    }
    if !input.is_empty() {
        return Err(Error);
    }
    Ok(Volume {
        roots: links,
        volumes,
    })
}
fn put_blob(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), Error> {
    let n = u16::try_from(bytes.len()).map_err(|_| Error)?;
    out.extend(n.to_le_bytes());
    out.extend(bytes);
    Ok(())
}
fn put_time(out: &mut Vec<u8>, time: Option<i64>) -> Result<(), Error> {
    if time.is_some_and(|t| t == i64::MIN || Timestamp::from_unix_millis(t).is_none()) {
        return Err(Error);
    }
    out.extend(time.unwrap_or(i64::MIN).to_le_bytes());
    Ok(())
}
fn put_observation(out: &mut Vec<u8>, obs: &Observation<'_>) -> Result<(), Error> {
    match obs {
        Observation::NotAttempted => out.push(0),
        Observation::Success {
            observed_ms,
            total,
            used,
            free,
        } => {
            out.push(1);
            put_time(out, *observed_ms)?;
            for n in [total, used, free] {
                out.extend(n.to_le_bytes());
            }
        }
        Observation::Failure {
            observed_ms,
            kind,
            reason,
            os_code,
        } => {
            if !key(kind) || !key(reason) {
                return Err(Error);
            }
            out.push(2);
            put_time(out, *observed_ms)?;
            put_blob(out, kind.as_bytes())?;
            put_blob(out, reason.as_bytes())?;
            if let Some(code) = os_code {
                out.push(1);
                out.extend(code.to_le_bytes());
            } else {
                out.push(0);
            }
        }
    }
    Ok(())
}
fn observation_len(obs: &Observation<'_>) -> Result<usize, Error> {
    match obs {
        Observation::NotAttempted => Ok(1),
        Observation::Success { .. } => Ok(33),
        Observation::Failure {
            kind,
            reason,
            os_code,
            ..
        } => {
            u16::try_from(kind.len()).map_err(|_| Error)?;
            u16::try_from(reason.len()).map_err(|_| Error)?;
            14usize
                .checked_add(kind.len())
                .and_then(|n| n.checked_add(reason.len()))
                .and_then(|n| n.checked_add(if os_code.is_some() { 4 } else { 0 }))
                .ok_or(Error)
        }
    }
}
impl Volume<'_> {
    /// Encode a complete v1 body, rejecting unrepresentable values.
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let roots = u16::try_from(self.roots.len()).map_err(|_| Error)?;
        let count = u16::try_from(self.volumes.len()).map_err(|_| Error)?;
        let mut len = 5usize
            .checked_add(self.roots.len().checked_mul(6).ok_or(Error)?)
            .ok_or(Error)?;
        for v in &self.volumes {
            u16::try_from(v.id.len()).map_err(|_| Error)?;
            u16::try_from(v.basis.len()).map_err(|_| Error)?;
            len = len
                .checked_add(4)
                .and_then(|n| n.checked_add(v.id.len()))
                .and_then(|n| n.checked_add(v.basis.len()))
                .and_then(|n| n.checked_add(observation_len(&v.start).ok()?))
                .and_then(|n| n.checked_add(observation_len(&v.end).ok()?))
                .ok_or(Error)?;
            if len + 1 > 65_536 {
                return Err(Error);
            }
        }
        if len + 1 > 65_536 {
            return Err(Error);
        }
        let mut out = Vec::with_capacity(len);
        out.push(1);
        out.extend(roots.to_le_bytes());
        out.extend(count.to_le_bytes());
        for r in &self.roots {
            out.extend(r.volume_index.unwrap_or(u16::MAX).to_le_bytes());
            out.extend(r.item_number.unwrap_or(0).to_le_bytes());
        }
        for v in &self.volumes {
            put_blob(&mut out, v.id)?;
            put_blob(&mut out, v.basis.as_bytes())?;
            put_observation(&mut out, &v.start)?;
            put_observation(&mut out, &v.end)?;
            if out.len() + 1 > 65_536 {
                return Err(Error);
            }
        }
        let decoded = decode(&out, self.roots.len())?;
        if decoded != *self {
            return Err(Error);
        }
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_vectors_decode_independently() -> Result<(), Error> {
        let vectors: &[(&[u8], usize)] = &[
            (&[1, 0, 0, 0, 0], 0),
            (
                &[
                    1, 2, 0, 2, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0, 0, 0, 1, 0, 65, 0, 0, 0, 0, 1, 0,
                    66, 0, 0, 0, 0,
                ],
                2,
            ),
            (&[1, 1, 0, 1, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 1),
        ];
        let empty = decode(vectors[0].0, 0)?;
        assert!(empty.roots.is_empty() && empty.volumes.is_empty());
        let paired = decode(vectors[1].0, 2)?;
        assert_eq!(
            paired.roots,
            vec![
                RootLink {
                    volume_index: Some(0),
                    item_number: Some(1)
                },
                RootLink {
                    volume_index: Some(1),
                    item_number: Some(2)
                }
            ]
        );
        assert_eq!(
            paired.volumes.iter().map(|v| v.id).collect::<Vec<_>>(),
            vec![b"A".as_slice(), b"B".as_slice()]
        );
        assert!(paired.volumes.iter().all(|v| v.basis.is_empty()
            && v.start == Observation::NotAttempted
            && v.end == Observation::NotAttempted));
        let unknown = decode(vectors[2].0, 1)?;
        assert_eq!(
            unknown.roots,
            vec![RootLink {
                volume_index: None,
                item_number: None
            }]
        );
        assert_eq!(unknown.volumes[0].id, b"");
        assert_eq!(unknown.volumes[0].basis, "");
        assert_eq!(unknown.volumes[0].start, Observation::NotAttempted);
        assert_eq!(unknown.volumes[0].end, Observation::NotAttempted);
        for (bytes, count) in vectors {
            let decoded = decode(bytes, *count)?;
            assert_eq!(decoded.encode()?, *bytes);
        }
        Ok(())
    }
    #[test]
    fn rejects_corruption_and_preserves_unknown_values() -> Result<(), Error> {
        let volume = Volume {
            roots: vec![RootLink {
                volume_index: Some(0),
                item_number: Some(1),
            }],
            volumes: vec![VolumeEntry {
                id: b"\xff",
                basis: "future basis",
                start: Observation::Failure {
                    observed_ms: None,
                    kind: "future_error",
                    reason: "unknown.reason",
                    os_code: Some(-4),
                },
                end: Observation::NotAttempted,
            }],
        };
        let bytes = volume.encode()?;
        assert_eq!(decode(&bytes, 1)?, volume);
        for n in 0..bytes.len() {
            assert!(decode(&bytes[..n], 1).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode(&trailing, 1).is_err());
        let mut invalid = bytes;
        invalid[5] = 1;
        assert!(decode(&invalid, 1).is_err());
        Ok(())
    }
}
