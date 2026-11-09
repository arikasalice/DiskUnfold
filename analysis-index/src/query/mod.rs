//! In-memory, pinned-result query primitives. Text normalization and IPC belong to callers.
mod condition;
mod plan;
pub use condition::{Basis, Boundary, Condition, Kind, NameEncoding, NameTest, Predicate, Truth};

use crate::ResultVersion;
use du_core_contracts::result::{ResultItem, ResultItemKind, ResultVersionRead};
use du_core_contracts::{ItemRef, SizeAmount, SizeValue};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// One immutable item and its native root-relative path in a pinned result.
pub struct Record<'a> {
    pub item: &'a ResultItem,
    pub path: &'a [Vec<u8>],
    pub version: &'a ResultVersion,
}

impl<'a> Record<'a> {
    /// Reads the item's observed size, or its subtree aggregate for a directory.
    pub fn size(&self, basis: Basis) -> Option<&'a SizeValue> {
        if self.item.kind == ResultItemKind::Directory {
            let aggregate = self.version.space(self.item.item_no)?;
            return Some(match basis {
                Basis::Logical => &aggregate.logical,
                Basis::Allocated => &aggregate.allocated,
            });
        }
        match basis {
            Basis::Logical => self.item.size.as_ref(),
            Basis::Allocated => self
                .version
                .physical
                .as_ref()?
                .get(self.item.item_no as usize)?
                .as_ref()?
                .allocated
                .as_ref(),
        }
    }
}

/// Sortable native key, where `Missing` always sorts after known values.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortValue {
    Bytes(Vec<u8>),
    Path(Vec<Vec<u8>>),
    Number(u64),
    Missing,
}

/// A registered pure predicate evaluator; implementations must not mutate the result.
pub type PredicateEvaluator = for<'a> fn(&Record<'a>, &Predicate) -> Truth;
/// A registered pure sort-key extractor, with stable key type for a given strategy.
pub type SortEvaluator = for<'a> fn(&Record<'a>, Basis) -> SortValue;

/// Versioned in-memory registry for condition and ordering strategies.
#[derive(Default)]
pub struct Registry {
    predicates: BTreeMap<(String, u32), PredicateEvaluator>,
    sorts: BTreeMap<(String, u32), SortEvaluator>,
}

/// Invalid registration, unknown strategy, or an interrupted evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    DuplicateStrategy,
    UnknownPredicate,
    UnknownSort,
    InvalidCondition,
    Cancelled,
}

impl Registry {
    /// Creates the supported baseline strategies, each fixed at revision one.
    pub fn baseline() -> Self {
        let mut result = Self::default();
        result.predicates.insert(("pred.name".into(), 1), builtin);
        result.predicates.insert(("pred.type".into(), 1), builtin);
        result.predicates.insert(("pred.size".into(), 1), builtin);
        result.sorts.insert(("sort.name".into(), 1), sort_name);
        result.sorts.insert(("sort.path".into(), 1), sort_path);
        result.sorts.insert(("sort.size".into(), 1), sort_size);
        result
    }

    /// Registers one predicate revision; duplicate keys are rejected without replacement.
    pub fn register_predicate(
        &mut self,
        id: &str,
        revision: u32,
        evaluator: PredicateEvaluator,
    ) -> Result<(), Error> {
        let key = (id.to_owned(), revision);
        if id.is_empty() || revision == 0 || self.predicates.contains_key(&key) {
            return Err(Error::DuplicateStrategy);
        }
        self.predicates.insert(key, evaluator);
        Ok(())
    }

    /// Registers one sort-key revision; duplicate keys are rejected without replacement.
    pub fn register_sort(
        &mut self,
        id: &str,
        revision: u32,
        evaluator: SortEvaluator,
    ) -> Result<(), Error> {
        let key = (id.to_owned(), revision);
        if id.is_empty() || revision == 0 || self.sorts.contains_key(&key) {
            return Err(Error::DuplicateStrategy);
        }
        self.sorts.insert(key, evaluator);
        Ok(())
    }
}

fn builtin(record: &Record<'_>, predicate: &Predicate) -> Truth {
    match predicate {
        Predicate::Name(test, encoding) => condition::name(&record.item.name, test, *encoding),
        Predicate::Kind(kind) => condition::kind(record.item.kind, *kind),
        Predicate::Size(basis, boundary) => condition::size(record.size(*basis), *basis, *boundary),
        Predicate::Registered { .. } => Truth::Unknown,
    }
}

fn sort_name(record: &Record<'_>, _: Basis) -> SortValue {
    SortValue::Bytes(record.item.name.clone())
}
fn sort_path(record: &Record<'_>, _: Basis) -> SortValue {
    SortValue::Path(record.path.to_vec())
}
fn sort_size(record: &Record<'_>, basis: Basis) -> SortValue {
    let Some(size) = record.size(basis) else {
        return SortValue::Missing;
    };
    let expected = match basis {
        Basis::Logical => du_core_contracts::SizeBasis::Logical,
        Basis::Allocated => du_core_contracts::SizeBasis::Allocated,
    };
    if size.basis != expected || size.evidence == du_core_contracts::SizeEvidence::Unknown {
        return SortValue::Missing;
    }
    let bytes = match &size.amount {
        SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => bytes,
        SizeAmount::Unknown => return SortValue::Missing,
    };
    bytes
        .as_str()
        .parse()
        .map_or(SortValue::Missing, SortValue::Number)
}

/// Selection of a pinned sort strategy and ordering direction.
pub struct Sort<'a> {
    pub id: &'a str,
    pub revision: u32,
    pub basis: Basis,
    pub descending: bool,
}

/// A materialized selection retaining the source snapshot for its entire lifetime.
pub struct QueryResult {
    pub version: Arc<ResultVersion>,
    pub items: Vec<ItemRef>,
    pub unknown_count: u64,
    /// Normalized condition used by this evaluation.
    pub condition: Condition,
    /// Predicate IDs and revisions pinned for the evaluation.
    pub predicate_strategies: Vec<(String, u32)>,
    /// Sort-key ID and revision pinned for the evaluation.
    pub sort_strategy: (String, u32),
    /// Comparison metadata not available in the current result source.
    pub limitations: &'static [&'static str],
}

struct Row {
    item_no: u32,
    path: Vec<Vec<u8>>,
    key: SortValue,
}

/// Evaluates a condition against one pinned version. Cancellation returns no result set.
///
/// Name and path strategies compare native bytes literally; Unicode normalization and
/// locale or volume collation are not supplied by the current result source. In particular,
/// little-endian UTF-16 byte ordering is neither UTF-16 code-unit ordering nor display-name
/// collation. The caller must select the name encoding that matches the source.
pub fn run(
    registry: &Registry,
    version: Arc<ResultVersion>,
    condition: &Condition,
    sort: Sort<'_>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<QueryResult, Error> {
    plan::check_shape(condition, 0)?;
    let condition = condition.clone().normalize();
    let plan = plan::prepare(registry, &condition, 0)?;
    let sort_key = registry
        .sorts
        .get(&(sort.id.to_owned(), sort.revision))
        .ok_or(Error::UnknownSort)?;
    let mut rows = Vec::new();
    let mut unknown_count = 0;
    for item in version.items() {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let path = item_path(&version, item);
        let record = Record {
            item,
            path: &path,
            version: &version,
        };
        match plan.evaluate(&record) {
            Truth::Match => rows.push(Row {
                item_no: item.item_no,
                key: sort_key(&record, sort.basis),
                path,
            }),
            Truth::Unknown => unknown_count += 1,
            Truth::NoMatch => {}
        }
    }
    rows.sort_by(|a, b| compare_rows(a, b, sort.descending));
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let items = rows
        .into_iter()
        .map(|row| ItemRef {
            result_version: version.version_id().clone(),
            item_no: row.item_no,
        })
        .collect();
    let mut strategies = BTreeSet::new();
    collect_strategies(&condition, &mut strategies);
    Ok(QueryResult {
        version,
        items,
        unknown_count,
        condition,
        predicate_strategies: strategies.into_iter().collect(),
        sort_strategy: (sort.id.to_owned(), sort.revision),
        limitations: &[
            "query.volume_name_comparison_unavailable",
            "query.raw_path_order",
        ],
    })
}

fn collect_strategies(condition: &Condition, result: &mut BTreeSet<(String, u32)>) {
    match condition {
        Condition::Predicate(predicate) => {
            let (id, revision) = predicate.strategy();
            result.insert((id.to_owned(), revision));
        }
        Condition::All(children) | Condition::Any(children) => {
            for child in children {
                collect_strategies(child, result);
            }
        }
        Condition::Not(child) => collect_strategies(child, result),
    }
}

fn item_path(version: &ResultVersion, item: &ResultItem) -> Vec<Vec<u8>> {
    let mut path = Vec::new();
    let mut current = item;
    while let Some(parent) = current.parent {
        path.push(current.name.clone());
        let Some(next) = version.items().get(parent as usize) else {
            break;
        };
        current = next;
    }
    path.reverse();
    path
}

fn compare_rows(a: &Row, b: &Row, descending: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let order = match (&a.key, &b.key) {
        (SortValue::Missing, SortValue::Missing) => Ordering::Equal,
        (SortValue::Missing, _) => Ordering::Greater,
        (_, SortValue::Missing) => Ordering::Less,
        _ if descending => b.key.cmp(&a.key),
        _ => a.key.cmp(&b.key),
    };
    order
        .then_with(|| a.path.cmp(&b.path))
        .then_with(|| a.item_no.cmp(&b.item_no))
}
