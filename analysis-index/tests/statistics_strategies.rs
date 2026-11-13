use du_analysis_index::{
    statistics_distribution::strategies::{Error, Key, Registry, Request, SizeBins, Strategy},
    statistics_distribution::{self, Category, Row},
};
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, SizeValue};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn value(n: u64, lower: bool) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: if lower {
            SizeAmount::LowerBound { bytes: n.into() }
        } else {
            SizeAmount::Exact { bytes: n.into() }
        },
    }
}
fn request<'a>(rows: &'a [Row<'a>], bounds: &'a [u64], chunk_size: usize) -> Request<'a> {
    Request {
        rows,
        basis: &SizeBasis::Logical,
        boundaries: bounds,
        chunk_size,
    }
}

#[test]
fn registered_bins_matches_core_across_input_and_partition_sizes() -> Result<(), String> {
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
    let bounds = [0, 10, 20];
    let registry =
        statistics_distribution::strategies::bins_registry().map_err(|e| format!("{e:?}"))?;
    for count in 0..=rows.len() {
        let expected = statistics_distribution::bins(
            &rows[..count],
            &SizeBasis::Logical,
            &bounds,
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
        for chunk_size in [1, 2, 4, 20] {
            let actual = registry
                .compute(
                    "stat.bins",
                    None,
                    request(&rows[..count], &bounds, chunk_size),
                    &mut || false,
                )
                .map_err(|e| format!("{e:?}"))?;
            assert_eq!(
                actual.strategy,
                Key {
                    id: "stat.bins".into(),
                    revision: 1
                }
            );
            assert_eq!(actual.bins, expected);
        }
    }
    Ok(())
}

struct Counted(Arc<AtomicUsize>);
impl Strategy for Counted {
    fn calculate(
        &self,
        rows: &[Row<'_>],
        basis: &SizeBasis,
        bounds: &[u64],
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<statistics_distribution::strategies::Partial, Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        SizeBins.calculate(rows, basis, bounds, cancelled)
    }
    fn combine(
        &self,
        a: statistics_distribution::strategies::Partial,
        b: statistics_distribution::strategies::Partial,
    ) -> Result<statistics_distribution::strategies::Partial, Error> {
        SizeBins.combine(a, b)
    }
}

#[test]
fn selection_precedes_execution_and_cancellation_returns_no_model() -> Result<(), String> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = Registry::default();
    registry
        .register(
            Key {
                id: "stat.bins".into(),
                revision: 1,
            },
            Box::new(Counted(calls.clone())),
        )
        .map_err(|e| format!("{e:?}"))?;
    let value = value(10, false);
    let rows = [Row {
        size: Some(&value),
        category: Category::Missing,
    }];
    let bounds = [0, 10];
    assert_eq!(
        registry
            .compute("other", None, request(&rows, &bounds, 1), &mut || false)
            .err(),
        Some(Error::UnknownStrategy)
    );
    assert_eq!(
        registry
            .compute(
                "stat.bins",
                Some(2),
                request(&rows, &bounds, 1),
                &mut || false
            )
            .err(),
        Some(Error::UnknownStrategy)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut checks = 0;
    let result = registry.compute(
        "stat.bins",
        Some(1),
        request(&rows, &bounds, 1),
        &mut || {
            checks += 1;
            checks == 4
        },
    );
    assert_eq!(result.err(), Some(Error::Cancelled));
    Ok(())
}

#[test]
fn refuses_incompatible_partial_boundaries() -> Result<(), String> {
    let first = SizeBins
        .calculate(&[], &SizeBasis::Logical, &[0, 10], &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let second = SizeBins
        .calculate(&[], &SizeBasis::Logical, &[0, 20], &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        SizeBins.combine(first, second).err(),
        Some(Error::IncompatiblePartial)
    );
    Ok(())
}
