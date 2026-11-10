use super::*;

fn known<'a>(kind: &'a [u8], value: &'a [u8]) -> Observation<'a> {
    Observation {
        capability: Capability::Supported,
        kind: Some(kind),
        value: Some(value),
    }
}

#[test]
fn equal_and_different_values_require_the_same_kind() {
    let left = known(b"guid", b"id-1");
    assert_eq!(compare(left, known(b"guid", b"id-1")), Correspondence::Same);
    assert_eq!(
        compare(left, known(b"guid", b"id-2")),
        Correspondence::Different
    );
    assert_eq!(
        compare(left, known(b"uuid", b"id-1")),
        Correspondence::Unverifiable
    );
}

#[test]
fn missing_and_partial_evidence_never_creates_a_match() {
    let good = known(b"guid", b"id-1");
    for capability in [Capability::Partial, Capability::Unavailable] {
        let weak = Observation { capability, ..good };
        assert_eq!(compare(good, weak), Correspondence::Unverifiable);
        assert_eq!(compare(weak, good), Correspondence::Unverifiable);
    }
    for evidence in [
        known(b"", b"id-1"),
        known(b"guid", b""),
        Observation { kind: None, ..good },
        Observation {
            value: None,
            ..good
        },
    ] {
        assert_eq!(compare(good, evidence), Correspondence::Unverifiable);
    }
}

#[test]
fn duplicate_pairwise_matches_do_not_establish_unique_catalog_correspondence() {
    let catalog = known(b"guid", b"id-1");
    let candidates = [known(b"guid", b"id-1"), known(b"guid", b"id-1")];
    assert_eq!(
        candidates
            .iter()
            .filter(|candidate| compare(catalog, **candidate) == Correspondence::Same)
            .count(),
        2
    );
}

#[test]
fn comparison_is_symmetric_and_preserves_bytes() {
    let values = [
        known(b"uuid", &[0, 255]),
        known(b"uuid", &[0, 254]),
        known(b"other", &[0, 255]),
    ];
    for left in values {
        for right in values {
            assert_eq!(compare(left, right), compare(right, left));
        }
    }
}
