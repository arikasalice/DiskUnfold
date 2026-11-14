//! Member pages of one category in a fixed order.

use std::{collections::VecDeque, sync::Arc};

use du_core_contracts::{ItemRef, result::ResultVersionRead};

use super::{
    Classification, Decision, Error, REASON_RULE_UNEVALUABLE, REASON_VALUE_UNKNOWN,
    summary::{Tally, belongs},
};
use crate::query::{
    self, Basis, Record, Registry, Sort,
    sorting::{Order, OrderKeys},
};

/// At most this many (category, sort) orders are kept per classification.
const MAX_CACHED_ORDERS: usize = 16;
/// Files are examined this many apart between cancellation checks while ordering.
const CANCEL_INTERVAL: usize = 1024;

/// A member category derived from the rule decisions of one classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    /// Files that match the rule at this zero-based index.
    Group(usize),
    /// Files that match at least two rules.
    Overlap,
    /// Files that match no rule and have no undecided or unavailable rule.
    Unclassified,
    /// Files that match no rule and have an undecided or unavailable rule.
    Undecidable,
}

/// One position range of a category in its fixed order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// Number of files in the whole category.
    pub total: usize,
    /// The requested rows; fewer than requested at the end of the category.
    pub rows: Vec<PageRow>,
}

/// One observed file. A file in several groups is one row with several group indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRow {
    /// The file in the classified result version.
    pub item: ItemRef,
    /// Indices of the matched rules, in rule display order.
    pub groups: Vec<usize>,
    /// Known reasons for rules that could not decide for this file, sorted by key.
    pub undecidable_reasons: Vec<&'static str>,
}

/// Exact sort strategies and directions that define one member order.
type SortIdentity = Vec<(String, u32, Basis, bool)>;

/// Member orders computed so far, oldest first. An order lists file positions.
#[derive(Default)]
pub(super) struct OrderCache {
    entries: VecDeque<(Category, SortIdentity, Arc<[u32]>)>,
}

impl OrderCache {
    fn find(&self, category: Category, sorts: &SortIdentity) -> Option<Arc<[u32]>> {
        self.entries
            .iter()
            .find(|(cached, identity, _)| *cached == category && identity == sorts)
            .map(|(_, _, order)| Arc::clone(order))
    }

    /// Keeps the first order stored for a key when two readers computed it concurrently.
    fn insert(&mut self, category: Category, sorts: SortIdentity, order: Arc<[u32]>) -> Arc<[u32]> {
        if let Some(existing) = self.find(category, &sorts) {
            return existing;
        }
        if self.entries.len() >= MAX_CACHED_ORDERS {
            self.entries.pop_front();
        }
        self.entries
            .push_back((category, sorts, Arc::clone(&order)));
        order
    }
}

impl Classification {
    /// Reads `count` rows of `category` starting at position `start`.
    ///
    /// Rows follow `sorts` in priority order, and the last key is always the observed path
    /// order, so repeated requests return the same order. The order of a (category, sorts)
    /// pair is calculated on its first request and kept in this classification; the sorts are
    /// resolved from `registry` at that time, and an unknown strategy is rejected. A start
    /// beyond the end returns no rows and the unchanged total.
    ///
    /// # Errors
    /// Returns [`Error::InvalidSelection`] for a group index without a rule,
    /// [`Error::InvalidSort`] and [`Error::Cancelled`], which returns no partial page.
    pub fn page(
        &self,
        category: Category,
        sorts: &[Sort<'_>],
        registry: &Registry,
        start: usize,
        count: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Page, Error> {
        if matches!(category, Category::Group(index) if index >= self.rules.len()) {
            return Err(Error::InvalidSelection);
        }
        let order = self.member_order(category, sorts, registry, &mut cancelled)?;
        let end = start.saturating_add(count).min(order.len());
        let rows = order
            .get(start.min(end)..end)
            .unwrap_or_default()
            .iter()
            .map(|&position| self.page_row(position as usize))
            .collect::<Result<_, _>>()?;
        Ok(Page {
            total: order.len(),
            rows,
        })
    }

    fn page_row(&self, position: usize) -> Result<PageRow, Error> {
        let item_no = *self
            .files
            .get(position)
            .ok_or(Error::InconsistentMembership)?;
        let row = self.row(position);
        let tally = Tally::of(row);
        let mut undecidable_reasons = Vec::new();
        if tally.unavailable {
            undecidable_reasons.push(REASON_RULE_UNEVALUABLE);
        }
        if tally.unknown {
            undecidable_reasons.push(REASON_VALUE_UNKNOWN);
        }
        Ok(PageRow {
            item: ItemRef {
                result_version: self.version.version_id().clone(),
                item_no,
            },
            groups: row
                .iter()
                .enumerate()
                .filter(|(_, decision)| **decision == Decision::Match)
                .map(|(index, _)| index)
                .collect(),
            undecidable_reasons,
        })
    }

    fn member_order(
        &self,
        category: Category,
        sorts: &[Sort<'_>],
        registry: &Registry,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Arc<[u32]>, Error> {
        let identity: SortIdentity = sorts
            .iter()
            .map(|sort| {
                (
                    sort.id.to_owned(),
                    sort.revision,
                    sort.basis,
                    sort.descending,
                )
            })
            .collect();
        let cached = self.orders.lock().map_err(|_| Error::Unavailable)?;
        let known = cached.find(category, &identity);
        drop(cached);
        if let Some(order) = known {
            return Ok(order);
        }
        let order = self.compute_order(category, sorts, registry, cancelled)?;
        let mut cache = self.orders.lock().map_err(|_| Error::Unavailable)?;
        Ok(cache.insert(category, identity, order.into()))
    }

    /// Orders the category's files by the sort keys, then by observed path order.
    fn compute_order(
        &self,
        category: Category,
        sorts: &[Sort<'_>],
        registry: &Registry,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Vec<u32>, Error> {
        let order = Order::resolve(registry, sorts).map_err(Error::InvalidSort)?;
        let mut keyed: Vec<(Option<OrderKeys>, u32)> = Vec::new();
        for (position, &item_no) in self.files.iter().enumerate() {
            if position.is_multiple_of(CANCEL_INTERVAL) && cancelled() {
                return Err(Error::Cancelled);
            }
            if !belongs(category, self.row(position)) {
                continue;
            }
            let keys = if sorts.is_empty() {
                None
            } else {
                Some(self.sort_keys(&order, item_no)?)
            };
            let position = u32::try_from(position).map_err(|_| Error::InconsistentMembership)?;
            keyed.push((keys, position));
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        keyed.sort_by(|(left, a), (right, b)| {
            let by_keys = match (left, right) {
                (Some(left), Some(right)) => order.compare(left, right),
                _ => std::cmp::Ordering::Equal,
            };
            by_keys.then_with(|| a.cmp(b))
        });
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(keyed.into_iter().map(|(_, position)| position).collect())
    }

    fn sort_keys(&self, order: &Order<'_>, item_no: u32) -> Result<OrderKeys, Error> {
        let item = self
            .version
            .items()
            .get(item_no as usize)
            .ok_or(Error::Source(crate::QueryError::NotFound))?;
        let path = query::navigation_path(&self.version, item, None);
        Ok(order.keys(&Record {
            item,
            path: &path,
            version: &self.version,
        }))
    }
}
