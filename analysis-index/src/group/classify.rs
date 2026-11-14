//! Rule preparation and the chunked, order-preserving evaluation of a scope.

use std::{
    collections::BTreeSet,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use du_core_contracts::{ItemRef, result::ResultVersionRead};

use super::{
    Decision, Error,
    classification::{Classification, Parts},
    scope,
};
use crate::{
    QueryError, ResultVersion,
    query::{self, Condition, Record, Registry, plan::Plan},
};

const DEFAULT_CHUNK_ITEMS: usize = 4096;
const MAX_DEFAULT_WORKERS: usize = 4;

/// Cancellation and progress hooks shared by every worker of one classification.
///
/// Both methods may be called from several threads at once.
pub trait Control: Sync {
    /// Returns true once the classification should stop. Workers check it after each chunk.
    fn cancelled(&self) -> bool;
    /// Receives the number of files evaluated so far. Reported values never decrease.
    fn progress(&self, completed_files: u64);
}

/// Work division of one classification. The result never depends on these values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parallelism {
    /// Files evaluated between two cancellation checks and progress reports.
    pub chunk_items: NonZeroUsize,
    /// Threads that evaluate chunks, including the calling thread.
    pub workers: NonZeroUsize,
}

impl Default for Parallelism {
    /// Chunks of 4096 files and the available parallelism, at most four workers.
    fn default() -> Self {
        let limit = NonZeroUsize::new(MAX_DEFAULT_WORKERS).unwrap_or(NonZeroUsize::MIN);
        Self {
            chunk_items: NonZeroUsize::new(DEFAULT_CHUNK_ITEMS).unwrap_or(NonZeroUsize::MIN),
            workers: thread::available_parallelism().map_or(NonZeroUsize::MIN, |n| n.min(limit)),
        }
    }
}

/// Classifies the files below `root` (or the whole result) with `rules` in display order.
///
/// `root` is `None` for every scope root of the result, including a forest, or a folder of
/// `version`. The files are gathered in observed path order, split into chunks and
/// evaluated by `parallelism.workers` threads; the pieces are joined in path order, so the
/// result is identical for every chunk size and worker count. Each rule's predicates are
/// resolved once from `registry`; a rule with an unregistered predicate is unavailable for
/// every file. `control` is polled after each chunk, and a cancellation returns
/// [`Error::Cancelled`] with no partial classification.
///
/// # Errors
/// Returns [`Error::InvalidRoot`] for a root that is not a folder of `version`,
/// [`Error::InvalidRule`] for a rule beyond the condition limits and [`Error::Cancelled`].
pub fn classify_scope(
    version: Arc<ResultVersion>,
    root: Option<&ItemRef>,
    registry: &Registry,
    rules: Vec<Condition>,
    parallelism: Parallelism,
    control: &dyn Control,
) -> Result<Classification, Error> {
    scope::validate_root(&version, root)?;
    let plans = prepare_rules(registry, &rules)?;
    let rule_unavailable: Vec<bool> = plans.iter().map(Option::is_none).collect();
    if control.cancelled() {
        return Err(Error::Cancelled);
    }
    let scope = scope::collect(&version, root, control)?;
    let decisions = evaluate(&version, &scope.files, &plans, parallelism, control)?;
    drop(plans);
    if control.cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(Classification::assemble(Parts {
        version,
        root: root.cloned(),
        predicates: leaf_strategies(&rules),
        rules,
        rule_unavailable,
        files: scope.files,
        decisions,
        coverage_complete: scope.coverage_complete,
    }))
}

/// Resolves every rule once. `None` marks a rule with a predicate the registry lacks.
fn prepare_rules<'a>(
    registry: &Registry,
    rules: &'a [Condition],
) -> Result<Vec<Option<Plan<'a>>>, Error> {
    rules
        .iter()
        .map(|rule| {
            query::validate_condition(rule).map_err(Error::InvalidRule)?;
            match query::plan::prepare(registry, rule, 0) {
                Ok(plan) => Ok(Some(plan)),
                Err(query::Error::UnknownPredicate) => Ok(None),
                Err(error) => Err(Error::InvalidRule(error)),
            }
        })
        .collect()
}

/// Lists the leaf predicates of all rules, sorted and without duplicates.
fn leaf_strategies(rules: &[Condition]) -> Vec<(String, u32)> {
    let mut strategies = BTreeSet::new();
    for rule in rules {
        query::collect_strategies(rule, &mut strategies);
    }
    strategies.into_iter().collect()
}

/// The decisions of one chunk, tagged with the chunk's position in path order.
type Piece = (usize, Vec<Decision>);

/// State that every worker reads or advances while evaluating one scope.
struct Shared<'a> {
    version: &'a ResultVersion,
    files: &'a [u32],
    plans: &'a [Option<Plan<'a>>],
    chunk_items: usize,
    chunks: usize,
    next_chunk: AtomicUsize,
    stop: AtomicBool,
    completed: Mutex<u64>,
    control: &'a dyn Control,
}

fn evaluate(
    version: &ResultVersion,
    files: &[u32],
    plans: &[Option<Plan<'_>>],
    parallelism: Parallelism,
    control: &dyn Control,
) -> Result<Vec<Decision>, Error> {
    let chunk_items = parallelism.chunk_items.get();
    let chunks = files.len().div_ceil(chunk_items);
    let shared = Shared {
        version,
        files,
        plans,
        chunk_items,
        chunks,
        next_chunk: AtomicUsize::new(0),
        stop: AtomicBool::new(false),
        completed: Mutex::new(0),
        control,
    };
    let workers = parallelism.workers.get().min(chunks);
    let mut pieces = Vec::new();
    let mut failure = None;
    for outcome in run(&shared, workers) {
        match outcome {
            Ok(part) => pieces.extend(part),
            Err(error) => failure = Some(prefer_cancelled(failure, error)),
        }
    }
    if let Some(error) = failure {
        return Err(error);
    }
    if shared.stop.load(Ordering::SeqCst) {
        return Err(Error::Cancelled);
    }
    pieces.sort_by_key(|(index, _)| *index);
    let mut decisions = Vec::with_capacity(files.len().saturating_mul(plans.len()));
    for (_, piece) in pieces {
        decisions.extend(piece);
    }
    Ok(decisions)
}

/// Runs the calling thread and `workers - 1` helpers, each pulling the next free chunk.
/// A helper that cannot be started only reduces parallelism.
fn run(shared: &Shared<'_>, workers: usize) -> Vec<Result<Vec<Piece>, Error>> {
    if workers <= 1 {
        return vec![shared.work()];
    }
    thread::scope(|scope| {
        let helpers: Vec<_> = (1..workers)
            .filter_map(|_| {
                thread::Builder::new()
                    .spawn_scoped(scope, || shared.work())
                    .ok()
            })
            .collect();
        let mut outcomes = vec![shared.work()];
        for helper in helpers {
            outcomes.push(helper.join().unwrap_or(Err(Error::Unavailable)));
        }
        outcomes
    })
}

fn prefer_cancelled(current: Option<Error>, next: Error) -> Error {
    match current {
        Some(Error::Cancelled) => Error::Cancelled,
        _ if next == Error::Cancelled => next,
        Some(first) => first,
        None => next,
    }
}

impl Shared<'_> {
    /// Evaluates chunks until none is left or another worker stopped the run.
    fn work(&self) -> Result<Vec<Piece>, Error> {
        let mut pieces = Vec::new();
        while !self.stop.load(Ordering::SeqCst) {
            let index = self.next_chunk.fetch_add(1, Ordering::SeqCst);
            if index >= self.chunks {
                break;
            }
            let start = index.saturating_mul(self.chunk_items);
            let end = start.saturating_add(self.chunk_items).min(self.files.len());
            let items = self.files.get(start..end).unwrap_or_default();
            let step = self
                .evaluate_chunk(items)
                .and_then(|decisions| self.finish_chunk(items.len()).map(|()| decisions));
            match step {
                Ok(decisions) => pieces.push((index, decisions)),
                Err(error) => {
                    self.stop.store(true, Ordering::SeqCst);
                    return Err(error);
                }
            }
        }
        Ok(pieces)
    }

    fn evaluate_chunk(&self, items: &[u32]) -> Result<Vec<Decision>, Error> {
        let mut decisions = Vec::with_capacity(items.len().saturating_mul(self.plans.len()));
        for &item_no in items {
            let item = self
                .version
                .items()
                .get(item_no as usize)
                .ok_or(Error::Source(QueryError::NotFound))?;
            let path = query::navigation_path(self.version, item, None);
            let record = Record {
                item,
                path: &path,
                version: self.version,
            };
            decisions.extend(self.plans.iter().map(|plan| {
                plan.as_ref().map_or(Decision::RuleUnavailable, |plan| {
                    plan.evaluate(&record).into()
                })
            }));
        }
        Ok(decisions)
    }

    /// Checks for cancellation, then reports the running file count in order.
    fn finish_chunk(&self, files: usize) -> Result<(), Error> {
        if self.control.cancelled() {
            return Err(Error::Cancelled);
        }
        let mut completed = self.completed.lock().map_err(|_| Error::Unavailable)?;
        *completed = completed.saturating_add(u64::try_from(files).unwrap_or(u64::MAX));
        self.control.progress(*completed);
        Ok(())
    }
}
