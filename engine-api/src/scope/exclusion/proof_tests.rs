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
        rules: vec![BoundRule {
            original: ScopeExclusion::Path {
                path: original.clone(),
            },
            key: ScopeExclusion::Path { path: key.clone() },
        }],
        boundary: ScopeBoundary::CurrentVolume,
        included_mounts: vec![],
        source: ScopeSource::Gui,
        group: du_core_contracts::volume::RootVolumeGroupObservation::NotApplicable,
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
    proof.rules[0].key = ScopeExclusion::Path {
        path: path("c:\\fixture\\other"),
    };
    assert_ne!(proof.fingerprint()?, fingerprint);
    assert_eq!(
        proof.fingerprint()?,
        digest::scope(&proof.draft(true))?.to_hex().to_string()
    );
    Ok(())
}

struct Literal;

impl PathComparison for Literal {
    fn path(&self, path: &OsPath) -> Option<ComparisonPath> {
        let raw = path.raw.bytes().ok()?;
        let (units, _) = raw.as_chunks::<2>();
        let text = String::from_utf16(
            &units
                .iter()
                .map(|u| u16::from_le_bytes(*u))
                .collect::<Vec<_>>(),
        )
        .ok()?;
        Some(ComparisonPath {
            volume: vec![1],
            components: text
                .split('\\')
                .map(|part| part.as_bytes().to_vec())
                .collect(),
            serialized_key: path.clone(),
        })
    }

    fn component(&self, _: &ComparisonPath, _: &[Vec<u8>], raw: &[u8]) -> Option<Vec<u8>> {
        Some(raw.to_vec())
    }
}

#[test]
fn a_resent_projected_label_does_not_replace_the_native_original() -> Result<(), EngineError> {
    let native = path(r"\\?\C:\Fixture\Secret");
    let resent = OsPath {
        display: r"C:\Fixture\Secret".into(),
        ..native.clone()
    };
    let rules =
        proven::ProvenRules::admit(Arc::new(Literal), &[ScopeExclusion::Path { path: resent }])?;
    let frozen = rules
        .freeze(&[(path(r"\\?\C:\Fixture"), vec![1])])
        .ok_or_else(|| EngineError::reason("support.internal_error"))?;
    let ScopeExclusion::Path { path: original } = &frozen.rules[0].original else {
        return Err(EngineError::reason("support.internal_error"));
    };
    assert_eq!(original.display, native.display);
    assert_eq!(original.raw, native.raw);
    Ok(())
}
