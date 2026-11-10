//! Authenticated continuation for a pinned failure filter, separate from query paging.
use du_core_contracts::{
    PathBytes,
    scan_failures::{ScanFailureKind, ScanFailuresRequest},
};

use crate::error::EngineError;

const PAYLOAD_LEN: usize = 25;
const DOMAIN: &[u8] = b"diskunfold.scan.failures.sf1\0";

pub(super) fn start(key: &[u8; 32], request: &ScanFailuresRequest) -> Result<usize, EngineError> {
    let Some(token) = &request.cursor else {
        return Ok(0);
    };
    if token.len() > 2_048 {
        return Err(invalid());
    }
    let mut parts = token.split('.');
    if parts.next() != Some("sf1") {
        return Err(invalid());
    }
    let payload: [u8; PAYLOAD_LEN] = decode(parts.next())?;
    let signature: [u8; 32] = decode(parts.next())?;
    // BLAKE3 Hash equality for a 32-byte array is constant-time.
    if parts.next().is_some() || mac(key, &payload) != signature {
        return Err(invalid());
    }
    if payload[..16] != version(request)? {
        return Err(EngineError::reason("result.requery_required"));
    }
    if payload[16] != filter(request.kind) {
        return Err(invalid());
    }
    let position = u64::from_le_bytes(payload[17..].try_into().map_err(|_| invalid())?);
    usize::try_from(position).map_err(|_| invalid())
}

pub(super) fn encode(
    key: &[u8; 32],
    request: &ScanFailuresRequest,
    position: u64,
) -> Result<String, EngineError> {
    let mut payload = [0; PAYLOAD_LEN];
    payload[..16].copy_from_slice(&version(request)?);
    payload[16] = filter(request.kind);
    payload[17..].copy_from_slice(&position.to_le_bytes());
    Ok(format!(
        "sf1.{}.{}",
        PathBytes::from_bytes(&payload).as_str(),
        PathBytes::from_bytes(mac(key, &payload).as_bytes()).as_str(),
    ))
}

fn version(request: &ScanFailuresRequest) -> Result<[u8; 16], EngineError> {
    uuid::Uuid::parse_str(request.result_version.as_str())
        .map(|value| *value.as_bytes())
        .map_err(|_| invalid())
}

fn filter(kind: Option<ScanFailureKind>) -> u8 {
    match kind {
        None => 0,
        Some(ScanFailureKind::CollectionFailed) => 1,
        Some(ScanFailureKind::UnreadRange) => 2,
        Some(ScanFailureKind::Uncertain) => 3,
        Some(ScanFailureKind::UntraversedLink) => 4,
    }
}

fn decode<const N: usize>(part: Option<&str>) -> Result<[u8; N], EngineError> {
    PathBytes::parse(part.ok_or_else(invalid)?)
        .map_err(|_| invalid())?
        .bytes()
        .map_err(|_| invalid())?
        .try_into()
        .map_err(|_| invalid())
}

fn mac(key: &[u8; 32], payload: &[u8; PAYLOAD_LEN]) -> blake3::Hash {
    let mut hasher = blake3::Hasher::new_keyed(key);
    hasher.update(DOMAIN);
    hasher.update(payload);
    hasher.finalize()
}

fn invalid() -> EngineError {
    EngineError::reason("cursor.invalid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::Id;

    #[test]
    fn authenticates_before_comparing_version_and_filter() -> Result<(), String> {
        let key = [9; 32];
        let mut request = ScanFailuresRequest {
            result_version: Id::parse("018f0000-0000-7000-8000-000000000001")?,
            kind: Some(ScanFailureKind::UnreadRange),
            cursor: None,
            count: None,
        };
        let token = encode(&key, &request, 8).map_err(|error| format!("{error:?}"))?;
        assert!(token.starts_with("sf1."));
        request.cursor = Some(token);
        assert_eq!(start(&key, &request), Ok(8));
        request.kind = None;
        assert_eq!(start(&key, &request), Err(invalid()));
        request.kind = Some(ScanFailureKind::UnreadRange);
        request.result_version = Id::parse("018f0000-0000-7000-8000-000000000002")?;
        assert_eq!(start(&[7; 32], &request), Err(invalid()));
        assert_eq!(
            start(&key, &request),
            Err(EngineError::reason("result.requery_required"))
        );
        if let Some(token) = request.cursor.as_mut() {
            token.push('=');
        }
        assert_eq!(start(&key, &request), Err(invalid()));
        request.cursor = Some("x".repeat(2_049));
        assert_eq!(start(&key, &request), Err(invalid()));
        Ok(())
    }
}
