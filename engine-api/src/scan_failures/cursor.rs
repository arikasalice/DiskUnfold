//! Authenticated continuation for a pinned failure filter, separate from query paging.
use du_core_contracts::{
    PathBytes,
    scan_failures::{ScanFailureKind, ScanFailuresRequest},
};

use crate::error::EngineError;

const PAYLOAD_LEN: usize = 58;
const DOMAIN: &[u8] = b"diskunfold.scan.failures.sf2\0";

pub(super) fn start(key: &[u8; 32], request: &ScanFailuresRequest) -> Result<usize, EngineError> {
    let Some(payload) = authenticated_payload(key, request)? else {
        return Ok(0);
    };
    ensure_version(&payload, request)?;
    if payload[16] != filter(request.kind) {
        return Err(invalid());
    }
    if payload[17] != u8::from(request.failures_only.unwrap_or(false))
        || payload[18..50] != roots_filter(request)
    {
        return Err(invalid());
    }
    let position = u64::from_le_bytes(payload[50..].try_into().map_err(|_| invalid())?);
    usize::try_from(position).map_err(|_| invalid())
}

pub(super) fn check_version(
    key: &[u8; 32],
    request: &ScanFailuresRequest,
) -> Result<(), EngineError> {
    if let Some(payload) = authenticated_payload(key, request)? {
        ensure_version(&payload, request)?;
    }
    Ok(())
}

fn authenticated_payload(
    key: &[u8; 32],
    request: &ScanFailuresRequest,
) -> Result<Option<[u8; PAYLOAD_LEN]>, EngineError> {
    let Some(token) = &request.cursor else {
        return Ok(None);
    };
    if token.len() > 2_048 {
        return Err(invalid());
    }
    let mut parts = token.split('.');
    if parts.next() != Some("sf2") {
        return Err(invalid());
    }
    let payload: [u8; PAYLOAD_LEN] = decode(parts.next())?;
    let signature: [u8; 32] = decode(parts.next())?;
    // BLAKE3 Hash equality for a 32-byte array is constant-time.
    if parts.next().is_some() || mac(key, &payload) != signature {
        return Err(invalid());
    }
    Ok(Some(payload))
}

fn ensure_version(
    payload: &[u8; PAYLOAD_LEN],
    request: &ScanFailuresRequest,
) -> Result<(), EngineError> {
    if payload[..16] != version(request)? {
        return Err(EngineError::reason("cursor.stale"));
    }
    Ok(())
}

pub(super) fn encode(
    key: &[u8; 32],
    request: &ScanFailuresRequest,
    position: u64,
) -> Result<String, EngineError> {
    let mut payload = [0; PAYLOAD_LEN];
    payload[..16].copy_from_slice(&version(request)?);
    payload[16] = filter(request.kind);
    payload[17] = u8::from(request.failures_only.unwrap_or(false));
    payload[18..50].copy_from_slice(&roots_filter(request));
    payload[50..].copy_from_slice(&position.to_le_bytes());
    Ok(format!(
        "sf2.{}.{}",
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

fn roots_filter(request: &ScanFailuresRequest) -> [u8; 32] {
    // The roots and the folder the rows are restricted to (F02 #358) bind a cursor
    // together: a cursor made for one folder is refused for another.
    let mut hasher = blake3::Hasher::new_derive_key("diskunfold.scan.failures.roots.sf2");
    match &request.root_indexes {
        None => {
            hasher.update(&[0]);
        }
        Some(indexes) => {
            hasher.update(&[1]);
            hasher.update(&(indexes.len() as u32).to_le_bytes());
            for index in indexes {
                hasher.update(&index.to_le_bytes());
            }
        }
    };
    match &request.under_item {
        None => {
            hasher.update(&[0]);
        }
        Some(item) => {
            hasher.update(&[1]);
            hasher.update(&item.item_no.to_le_bytes());
        }
    }
    *hasher.finalize().as_bytes()
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
    crate::cursor_mac::mac(key, DOMAIN, payload)
}

fn invalid() -> EngineError {
    EngineError::reason("cursor.invalid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{Id, ItemRef};

    #[test]
    fn authenticates_before_comparing_version_and_filter() -> Result<(), String> {
        let key = [9; 32];
        let mut request = ScanFailuresRequest {
            result_version: Id::parse("018f0000-0000-7000-8000-000000000001")?,
            kind: Some(ScanFailureKind::UnreadRange),
            root_indexes: Some(vec![0, 3]),
            failures_only: Some(true),
            under_item: None,
            cursor: None,
            count: None,
        };
        let token = encode(&key, &request, 8).map_err(|error| format!("{error:?}"))?;
        assert!(token.starts_with("sf2."));
        request.cursor = Some(token);
        assert_eq!(start(&key, &request), Ok(8));
        request.kind = None;
        assert_eq!(start(&key, &request), Err(invalid()));
        request.kind = Some(ScanFailureKind::UnreadRange);
        request.root_indexes = Some(vec![0]);
        assert_eq!(start(&key, &request), Err(invalid()));
        request.root_indexes = Some(vec![0, 3]);
        request.under_item = Some(ItemRef {
            result_version: request.result_version.clone(),
            item_no: 4,
        });
        assert_eq!(start(&key, &request), Err(invalid()));
        request.under_item = None;
        request.failures_only = Some(false);
        assert_eq!(start(&key, &request), Err(invalid()));
        request.failures_only = Some(true);
        request.result_version = Id::parse("018f0000-0000-7000-8000-000000000002")?;
        assert_eq!(start(&[7; 32], &request), Err(invalid()));
        assert_eq!(
            start(&key, &request),
            Err(EngineError::reason("cursor.stale"))
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
