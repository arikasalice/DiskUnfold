//! Resolve ordered sort strategies once and compare their native keys.
use std::cmp::Ordering;

use super::{Basis, Error, Registry, Row, SortEvaluator, SortValue};

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
    for ((left, right), sort) in a.keys.iter().zip(&b.keys).zip(sorts) {
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
    a.path.cmp(&b.path).then_with(|| a.item_no.cmp(&b.item_no))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_number_breaks_a_fully_tied_native_path() {
        let first = Row {
            item_no: 2,
            path: vec![b"same".to_vec()],
            keys: Vec::new(),
        };
        let second = Row {
            item_no: 3,
            path: first.path.clone(),
            keys: Vec::new(),
        };
        assert_eq!(compare(&first, &second, &[]), Ordering::Less);
        assert_eq!(compare(&second, &first, &[]), Ordering::Greater);
    }
}
