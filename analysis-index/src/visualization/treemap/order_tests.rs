use super::*;
use crate::{IngestSink, Observation};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    result::{ResultNativePath, ResultScopeRoot},
};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn root(path: &[u8], volume: u8) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(path),
        },
        volume_id: vec![volume],
        file_id: vec![1],
    }
}

/// Roots are the siblings below a forest's virtual top level, so they are ordered by
/// their scope-root number even though no treemap parent names them today.
#[test]
fn equal_size_scope_roots_sort_by_root_number() -> Result<(), String> {
    let sink = IngestSink::default();
    // Root paths are deliberately out of byte order.
    let roots = vec![root(b"/c", 1), root(b"/a", 2), root(b"/b", 3)];
    let mut build = sink
        .open_forest(id(1)?, roots)
        .map_err(|e| format!("{e:?}"))?;
    for index in 0..3 {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
        build
            .add_file_at(
                index,
                Observation {
                    path: vec![b"file".to_vec()],
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact { bytes: 10.into() },
                    }),
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(version.scope_roots().len(), 3);
    let rows = |bytes: [u64; 3], numbers: [u32; 3]| -> Result<Vec<AdmittedChild>, String> {
        numbers
            .into_iter()
            .map(|no| {
                Ok(AdmittedChild {
                    child: Child {
                        item: reference(&version, no),
                        value: value(&version, no, &SizeBasis::Logical)
                            .map_err(|e| format!("{e:?}"))?,
                        area_attributed: true,
                    },
                    bytes: bytes[no as usize],
                })
            })
            .collect()
    };
    let numbers = |rows: Result<Vec<AdmittedChild>, AdmissionError>| -> Result<Vec<u32>, String> {
        Ok(rows
            .map_err(|e| format!("{e:?}"))?
            .iter()
            .map(|row| row.child.item.item_no)
            .collect())
    };
    let equal = order(&version, rows([10; 3], [2, 0, 1])?, &mut || false);
    assert_eq!(numbers(equal)?, [0, 1, 2]);
    // A larger root still comes first; equal roots keep root-number order.
    let mixed = order(&version, rows([10, 20, 10], [2, 0, 1])?, &mut || false);
    assert_eq!(numbers(mixed)?, [1, 0, 2]);
    Ok(())
}
