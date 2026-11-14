//! Resolve ordered sort strategies once and compare their native keys.
use std::cmp::Ordering;

use super::{Basis, Error, Record, Registry, Row, SortEvaluator, SortValue};

/// Selection of one exact registered revision and ordering direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort<'a> {
    pub id: &'a str,
    pub revision: u32,
    pub basis: Basis,
    pub descending: bool,
}

/// Owned sort selection retained with an immutable query result and its pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub id: String,
    pub revision: u32,
    pub basis: Basis,
    pub descending: bool,
}

impl From<&Sort<'_>> for SortSpec {
    fn from(sort: &Sort<'_>) -> Self {
        Self {
            id: sort.id.to_owned(),
            revision: sort.revision,
            basis: sort.basis,
            descending: sort.descending,
        }
    }
}

pub(super) struct Prepared<'a> {
    pub spec: Sort<'a>,
    pub evaluate: SortEvaluator,
}

/// One key is inline; only additional keys need per-row vector storage.
pub(super) struct Keys {
    first: SortValue,
    additional: Vec<SortValue>,
}

impl Keys {
    pub(super) fn evaluate(
        first: &Prepared<'_>,
        additional: &[Prepared<'_>],
        record: &Record<'_>,
    ) -> Self {
        Self {
            first: (first.evaluate)(record, first.spec.basis),
            additional: additional
                .iter()
                .map(|sort| (sort.evaluate)(record, sort.spec.basis))
                .collect(),
        }
    }

    fn iter(&self) -> impl Iterator<Item = &SortValue> {
        std::iter::once(&self.first).chain(&self.additional)
    }
}

/// Copies function pointers at admission so evaluation never re-reads the registry.
pub(super) fn prepare<'a>(
    registry: &Registry,
    sorts: &[Sort<'a>],
) -> Result<Vec<Prepared<'a>>, Error> {
    sorts
        .iter()
        .map(|sort| {
            let evaluate = *registry
                .sorts
                .get(&(sort.id.to_owned(), sort.revision))
                .ok_or(Error::UnknownSort)?;
            Ok(Prepared {
                spec: *sort,
                evaluate,
            })
        })
        .collect()
}

/// Missing values always follow known values; fully tied rows use native path and item number.
pub(super) fn compare(a: &Row, b: &Row, sorts: &[Prepared<'_>]) -> Ordering {
    compare_values(a.keys.iter(), b.keys.iter(), sorts)
        .then_with(|| a.path.cmp(&b.path))
        .then_with(|| a.item_no.cmp(&b.item_no))
}

/// Orders two key lists by the selected sorts; missing values follow known values.
fn compare_values<'v>(
    a: impl Iterator<Item = &'v SortValue>,
    b: impl Iterator<Item = &'v SortValue>,
    sorts: &[Prepared<'_>],
) -> Ordering {
    for ((left, right), sort) in a.zip(b).zip(sorts) {
        let order = match (left, right) {
            (SortValue::Missing, SortValue::Missing) => Ordering::Equal,
            (SortValue::Missing, _) => Ordering::Greater,
            (_, SortValue::Missing) => Ordering::Less,
            _ if sort.spec.descending => right.cmp(left),
            _ => left.cmp(right),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

/// Sort keys of one record, in the order of the selected sorts.
pub(crate) struct OrderKeys(Vec<SortValue>);

/// Sort strategies resolved once for ordering records that are not rows of a query result.
/// A caller that needs a total order supplies its own final tie-break.
pub(crate) struct Order<'a> {
    prepared: Vec<Prepared<'a>>,
}

impl<'a> Order<'a> {
    /// Resolves each exact registered revision; an unknown strategy is rejected.
    pub(crate) fn resolve(registry: &Registry, sorts: &[Sort<'a>]) -> Result<Self, Error> {
        Ok(Self {
            prepared: prepare(registry, sorts)?,
        })
    }

    /// Evaluates every selected sort key for one record.
    pub(crate) fn keys(&self, record: &Record<'_>) -> OrderKeys {
        OrderKeys(
            self.prepared
                .iter()
                .map(|sort| (sort.evaluate)(record, sort.spec.basis))
                .collect(),
        )
    }

    /// Compares two key lists; missing values always follow known values.
    pub(crate) fn compare(&self, a: &OrderKeys, b: &OrderKeys) -> Ordering {
        compare_values(a.0.iter(), b.0.iter(), &self.prepared)
    }
}

/// Avoids starting a potentially expensive sort after cancellation and checks again before returning rows.
pub(super) fn sort_rows(
    rows: &mut [Row],
    cancelled: &mut impl FnMut() -> bool,
    compare: impl FnMut(&Row, &Row) -> Ordering,
) -> Result<(), Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    rows.sort_by(compare);
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_before_sort_does_not_compare_rows() {
        let row = |item_no| Row {
            item_no,
            path: vec![],
            keys: Keys {
                first: SortValue::Missing,
                additional: Vec::new(),
            },
        };
        let mut rows = [row(2), row(1)];
        let mut comparisons = 0;
        let result = sort_rows(&mut rows, &mut || true, |a, b| {
            comparisons += 1;
            a.item_no.cmp(&b.item_no)
        });
        assert_eq!(result, Err(Error::Cancelled));
        assert_eq!(comparisons, 0);
        assert_eq!(rows[0].item_no, 2);
    }

    #[test]
    fn completed_sort_rechecks_cancellation_before_returning_rows() {
        let mut checks = 0;
        let mut rows = [];
        let result = sort_rows(
            &mut rows,
            &mut || {
                checks += 1;
                checks == 2
            },
            |_, _| Ordering::Equal,
        );
        assert_eq!(result, Err(Error::Cancelled));
        assert_eq!(checks, 2);
    }

    #[test]
    fn item_number_breaks_a_fully_tied_native_path() {
        let first = Row {
            item_no: 2,
            path: vec![b"same".to_vec()],
            keys: Keys {
                first: SortValue::Missing,
                additional: Vec::new(),
            },
        };
        let second = Row {
            item_no: 3,
            path: first.path.clone(),
            keys: Keys {
                first: SortValue::Missing,
                additional: Vec::new(),
            },
        };
        assert_eq!(compare(&first, &second, &[]), Ordering::Less);
        assert_eq!(compare(&second, &first, &[]), Ordering::Greater);
    }

    #[test]
    fn one_inline_key_has_no_additional_vector_allocation() -> Result<(), String> {
        use crate::IngestSink;
        use du_core_contracts::{
            Id,
            result::{ResultCompleteness, ResultVersionRead},
        };
        let id = |n| Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"));
        let sink = IngestSink::default();
        let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
        build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        let version = build
            .seal(id(2)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        let item = version.items().first().ok_or("missing root")?;
        let record = Record {
            item,
            path: &[],
            version: &version,
        };
        let sorts = [Sort {
            id: "sort.size",
            revision: 1,
            basis: Basis::Logical,
            descending: false,
        }];
        let prepared = prepare(&Registry::baseline(), &sorts).map_err(|e| format!("{e:?}"))?;
        let (first, other) = prepared.split_first().ok_or("missing strategy")?;
        let keys = Keys::evaluate(first, other, &record);
        assert_eq!(keys.additional.capacity(), 0);
        assert_eq!(keys.iter().count(), 1);
        Ok(())
    }
}
