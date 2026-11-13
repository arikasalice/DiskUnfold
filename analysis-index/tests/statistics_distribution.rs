use du_analysis_index::statistics_distribution::{self as distribution, Category, Error, Row};
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, SizeValue};

fn value(bytes: u64, lower: bool) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: if lower {
            SizeAmount::LowerBound {
                bytes: bytes.into(),
            }
        } else {
            SizeAmount::Exact {
                bytes: bytes.into(),
            }
        },
    }
}

#[test]
fn bins_preserve_boundary_unknown_lower_and_empty_rows() -> Result<(), String> {
    let values = [
        value(0, false),
        value(10, false),
        value(20, true),
        value(21, false),
    ];
    let mut rows: Vec<_> = values
        .iter()
        .map(|size| Row {
            size: Some(size),
            category: Category::Missing,
        })
        .collect();
    rows.push(Row {
        size: None,
        category: Category::Missing,
    });
    let result = distribution::bins(&rows, &SizeBasis::Logical, &[0, 10, 20], &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.intervals[0].count, 1);
    assert_eq!(
        (
            result.intervals[1].count,
            result.intervals[1].known_bytes,
            result.intervals[1].lower_bound_count
        ),
        (2, 30, 1)
    );
    assert_eq!((result.outside.count, result.outside.known_bytes), (1, 21));
    assert_eq!((result.unknown.count, result.unknown.unknown_count), (1, 1));
    let reversed: Vec<_> = rows
        .iter()
        .rev()
        .map(|row| Row {
            size: row.size,
            category: row.category.clone(),
        })
        .collect();
    assert_eq!(
        result,
        distribution::bins(&reversed, &SizeBasis::Logical, &[0, 10, 20], &mut || false)
            .map_err(|e| format!("{e:?}"))?
    );
    let empty = distribution::bins(&[], &SizeBasis::Logical, &[0, 10, 20], &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(empty.intervals.iter().all(|b| b.count == 0));
    for bounds in [&[][..], &[10][..], &[10, 10][..], &[20, 10][..]] {
        assert_eq!(
            distribution::bins(&[], &SizeBasis::Logical, bounds, &mut || false).err(),
            Some(Error::InvalidBoundaries)
        );
    }
    assert_eq!(
        distribution::bins(&rows, &SizeBasis::Allocated, &[0, 10], &mut || false).err(),
        Some(Error::BasisMismatch)
    );
    assert_eq!(
        distribution::bins(&rows, &SizeBasis::Logical, &[0, 10], &mut || true).err(),
        Some(Error::Cancelled)
    );
    Ok(())
}

#[test]
fn categories_distinguish_missing_no_extension_and_unclassified_deterministically()
-> Result<(), String> {
    let ten = value(10, false);
    let twenty = value(20, true);
    let rows = [
        Row {
            size: Some(&ten),
            category: Category::Key(b"txt".to_vec()),
        },
        Row {
            size: Some(&twenty),
            category: Category::Key(b"txt".to_vec()),
        },
        Row {
            size: None,
            category: Category::NoExtension,
        },
        Row {
            size: Some(&ten),
            category: Category::Missing,
        },
        Row {
            size: None,
            category: Category::Unclassified,
        },
    ];
    let result = distribution::by_type(&rows, &SizeBasis::Logical, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.len(), 4);
    let txt = result
        .iter()
        .find(|r| r.category == Category::Key(b"txt".to_vec()))
        .ok_or("missing txt")?;
    assert_eq!(
        (
            txt.amounts.count,
            txt.amounts.known_bytes,
            txt.amounts.lower_bound_count
        ),
        (2, 30, 1)
    );
    assert_eq!(
        result
            .iter()
            .find(|r| r.category == Category::NoExtension)
            .ok_or("no extension")?
            .amounts
            .unknown_count,
        1
    );
    let reversed: Vec<_> = rows.into_iter().rev().collect();
    assert_eq!(
        result,
        distribution::by_type(&reversed, &SizeBasis::Logical, &mut || false)
            .map_err(|e| format!("{e:?}"))?
    );
    assert!(
        distribution::by_type(&[], &SizeBasis::Logical, &mut || false)
            .map_err(|e| format!("{e:?}"))?
            .is_empty()
    );
    Ok(())
}
