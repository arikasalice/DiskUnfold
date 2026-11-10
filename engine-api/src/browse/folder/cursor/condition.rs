//! F04 normalized condition fingerprint bytes for the GUI session cursor.
use super::CursorError;
use du_analysis_index::query::{
    self, Basis, Boundary, Condition, Kind, NameEncoding, NameTest, Predicate,
};

const DOMAIN: &[u8] = b"diskunfold.query.condition.v1\0";

/// Computes the F04 model-v1 BLAKE3 fingerprint from normalized condition bytes.
pub struct CanonicalCondition;

impl CanonicalCondition {
    /// Returns the model-v1 fingerprint after bounded canonical normalization.
    pub fn fingerprint(&self, condition: &Condition) -> Result<[u8; 16], CursorError> {
        query::validate_condition(condition).map_err(|_| CursorError::FingerprintUnavailable)?;
        let normalized = condition.clone().normalize();
        query::validate_condition(&normalized).map_err(|_| CursorError::FingerprintUnavailable)?;
        let encoded = encode(&normalized)?;
        let mut hash = blake3::Hasher::new();
        hash.update(DOMAIN);
        hash.update(&encoded);
        let mut output = [0u8; 16];
        output.copy_from_slice(&hash.finalize().as_bytes()[..16]);
        Ok(output)
    }
}

fn encode(condition: &Condition) -> Result<Vec<u8>, CursorError> {
    let mut out = Vec::new();
    match condition {
        Condition::All(children) | Condition::Any(children) => {
            out.push(if matches!(condition, Condition::All(_)) {
                1
            } else {
                2
            });
            let mut parts = children.iter().map(encode).collect::<Result<Vec<_>, _>>()?;
            parts.sort();
            number(&mut out, parts.len())?;
            for part in parts {
                out.extend(part);
            }
        }
        Condition::Not(child) => {
            out.push(3);
            out.extend(encode(child)?);
        }
        Condition::Predicate(predicate) => {
            out.push(4);
            predicate_bytes(&mut out, predicate)?;
        }
    }
    Ok(out)
}

fn predicate_bytes(out: &mut Vec<u8>, predicate: &Predicate) -> Result<(), CursorError> {
    match predicate {
        Predicate::Name(test, encoding) => {
            out.push(1);
            let (kind, value) = match test {
                NameTest::Equal(v) => (1, v),
                NameTest::Contains(v) => (2, v),
                NameTest::StartsWith(v) => (3, v),
                NameTest::EndsWith(v) => (4, v),
            };
            out.push(kind);
            bytes(out, value)?;
            out.push(match encoding {
                NameEncoding::Bytes => 1,
                NameEncoding::WindowsUtf16Le => 2,
            });
        }
        Predicate::Kind(kind) => {
            out.push(2);
            out.push(match kind {
                Kind::File => 1,
                Kind::Directory => 2,
                Kind::Link => 3,
            });
        }
        Predicate::Size(basis, boundary) => {
            out.push(3);
            out.push(match basis {
                Basis::Logical => 1,
                Basis::Allocated => 2,
            });
            let (kind, value) = match boundary {
                Boundary::AtLeast(v) => (1, v),
                Boundary::GreaterThan(v) => (2, v),
                Boundary::AtMost(v) => (3, v),
                Boundary::LessThan(v) => (4, v),
            };
            out.push(kind);
            out.extend(value.to_le_bytes());
        }
        Predicate::Registered {
            id,
            revision,
            operand,
        } => {
            out.push(4);
            bytes(out, id.as_bytes())?;
            out.extend(revision.to_le_bytes());
            bytes(out, operand)?;
        }
    }
    Ok(())
}

fn number(out: &mut Vec<u8>, value: usize) -> Result<(), CursorError> {
    out.extend(
        u64::try_from(value)
            .map_err(|_| CursorError::FingerprintUnavailable)?
            .to_le_bytes(),
    );
    Ok(())
}

fn bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<(), CursorError> {
    number(out, value.len())?;
    out.extend(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_and_sort_canonical_encoded_children() -> Result<(), CursorError> {
        let kind = Condition::Predicate(Predicate::Kind(Kind::File));
        let name = Condition::Predicate(Predicate::Name(
            NameTest::Equal(b"a".to_vec()),
            NameEncoding::Bytes,
        ));
        let a = Condition::All(vec![kind.clone(), Condition::All(vec![name.clone()])]);
        let b = Condition::All(vec![name, kind]);
        assert_eq!(
            CanonicalCondition.fingerprint(&a)?,
            CanonicalCondition.fingerprint(&b)?
        );
        let mut expected = vec![1];
        expected.extend(2u64.to_le_bytes());
        expected.extend([4, 1, 1]);
        expected.extend(1u64.to_le_bytes());
        expected.extend([b'a', 1, 4, 2, 1]);
        assert_eq!(encode(&a.normalize())?, expected);
        Ok(())
    }

    #[test]
    fn rejects_deep_and_post_flatten_oversized_conditions() {
        let mut deep = Condition::All(vec![]);
        for _ in 0..65 {
            deep = Condition::Not(Box::new(deep));
        }
        assert_eq!(
            CanonicalCondition.fingerprint(&deep),
            Err(CursorError::FingerprintUnavailable)
        );
        let child = Condition::All(vec![Condition::Predicate(Predicate::Kind(Kind::File)); 256]);
        let large = Condition::All(vec![child.clone(), child]);
        assert_eq!(
            CanonicalCondition.fingerprint(&large),
            Err(CursorError::FingerprintUnavailable)
        );
    }

    #[test]
    fn encoded_byte_order_overrides_rust_enum_order() -> Result<(), CursorError> {
        let predicate = Condition::Predicate(Predicate::Kind(Kind::File));
        let negated = Condition::Not(Box::new(predicate.clone()));
        let encoded = encode(&Condition::All(vec![predicate, negated]).normalize())?;
        assert_eq!(&encoded[9..], &[3, 4, 2, 1, 4, 2, 1]);
        Ok(())
    }
}
