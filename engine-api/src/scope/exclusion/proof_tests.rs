use super::*;
use du_core_contracts::{
    PathBytes, PathEncoding,
    analysis_ipc::{ScopeBoundary, ScopeSource},
};

fn path(value: &str) -> OsPath {
    let bytes = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    OsPath {
        encoding: PathEncoding::WindowsUtf16le,
        raw: PathBytes::from_bytes(&bytes),
        display: value.into(),
        lossy: false,
    }
}

#[test]
fn original_and_fingerprint_are_derived_from_same_bound_conditions() -> Result<(), EngineError> {
    let original = path("C:\\Fixture\\Secret");
    let key = path("c:\\fixture\\secret");
    let mut proof = BoundScopeProof {
        root_pairs: vec![(path("C:\\Fixture"), path("c:\\fixture"))],
        rules: vec![VerifiedExclusion {
            original: original.clone(),
            comparison: ComparisonPath {
                volume: vec![1],
                components: vec![b"secret".to_vec()],
                serialized_key: key.clone(),
            },
        }],
        boundary: ScopeBoundary::CurrentVolume,
        included_mounts: vec![],
        source: ScopeSource::Gui,
    };
    assert_eq!(
        proof.draft(false).exclusions,
        vec![ScopeExclusion::Path { path: original }]
    );
    assert_eq!(
        proof.draft(true).exclusions,
        vec![ScopeExclusion::Path { path: key }]
    );
    let fingerprint = proof.fingerprint()?;
    proof.rules[0].comparison.serialized_key = path("c:\\fixture\\other");
    assert_ne!(proof.fingerprint()?, fingerprint);
    assert_eq!(
        proof.fingerprint()?,
        digest::scope(&proof.draft(true))?.to_hex().to_string()
    );
    Ok(())
}
