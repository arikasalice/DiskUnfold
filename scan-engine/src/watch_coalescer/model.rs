use std::collections::BTreeSet;

/// A relative path, represented as normalized comparison-key components.
/// Empty is the verified watch root. Components must already have been checked
/// against the selected scope, exclusions, and mount boundaries by the caller.
pub type Key = Vec<Vec<u8>>;

/// Immutable identity of one watch generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    /// Watch unit identifier (UUID bytes).
    pub watch_ref: [u8; 16],
    /// Selected scope revision identifier (UUID bytes).
    pub scope_revision: [u8; 16],
    /// Stable volume identity as provided by the scope owner.
    pub volume: Vec<u8>,
    /// Watch generation, starting at one.
    pub generation: u64,
}
/// Change hints only; no file contents or metadata are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Unknown,
    Create,
    Modify,
    Delete,
    Rename,
}
/// One adapter signal. File paths map to their parent directory; a directory
/// creation, deletion or rename also affects its own subtree. Rename partners
/// are supplied as two independent signals, even when a platform pairs them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signal {
    /// A validated relative path, or `None` if the adapter did not provide one.
    pub path: Option<Key>,
    /// Whether the directory subtree is affected, in addition to its parent.
    pub directory_subtree: bool,
    /// The adapter's neutral hint.
    pub kind: ChangeKind,
    /// Observation time in milliseconds from the injected clock.
    pub observed_at: u64,
}
/// One affected directory range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Normalized directory key within the watch root.
    pub path: Key,
    /// Whether the range covers descendants.
    pub subtree: bool,
    /// Union of hints observed for this range.
    pub kinds: BTreeSet<ChangeKind>,
    /// First and last signal times for the range, in milliseconds.
    pub first: u64,
    pub last: u64,
}
/// Gap reasons produced or carried by this coalescer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapReason {
    BeforeStart,
    SignalLost,
    Overflow,
    Unavailable,
    Stopped,
    PartialCapability,
    ConsumerResync,
}
/// One observation gap. The unit reference is provided by the enclosing batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gap {
    /// Per-unit gap sequence.
    pub id: u64,
    /// Gap classification.
    pub reason: GapReason,
    /// A directory subtree, or `None` for the whole watch unit.
    pub path: Option<Key>,
    /// Unknown start is distinct from time zero.
    pub start: Option<u64>,
    /// An absent end denotes an open gap.
    pub end: Option<u64>,
}
/// Whether this batch has a known gap in its observation evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    Normal,
    Gap,
}
/// One ordered internal handoff to the future F33 adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    /// Watch, scope, volume and generation bound to this batch.
    pub context: Context,
    /// Per-unit sequence shared with status events.
    pub sequence: u64,
    /// Sequence of change batches, independent of status events and generations.
    pub change_sequence: u64,
    /// Closed gaps replaced by this summary; empty for ordinary batches.
    pub replaced_gap_ids: Vec<u64>,
    /// Observation interval in injected-clock milliseconds.
    pub interval_start: u64,
    pub interval_end: u64,
    /// Sorted affected ranges; at most 32 per batch.
    pub changes: Vec<Change>,
    /// Gap evidence; for a split output, carried by the first batch.
    pub gaps: Vec<Gap>,
    /// Evidence state, including gaps carried by the first split batch.
    pub trust: Trust,
}
/// Last identifiers allocated by the owning per-unit registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    /// Last sequence shared by status events and change batches.
    pub sequence: u64,
    /// Last change-batch sequence across the unit lifetime.
    pub change_sequence: u64,
    /// Last gap identifier, including gaps no longer open.
    pub last_gap_id: u64,
}
/// Bounded-buffer settings. Capacities are provisional values in F18 DLD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Milliseconds after the first signal (published value: 1,000).
    pub window_ms: u64,
    /// Promote sibling keys when this threshold is reached (provisional: 64).
    pub promote_at: usize,
    /// Maximum key count: reaching this limit produces an overflow gap (provisional: 4,096).
    pub queue_limit: usize,
    /// Maximum number of gaps carried by one output batch.
    pub gap_limit: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            window_ms: 1000,
            promote_at: 64,
            queue_limit: 4096,
            gap_limit: 16,
        }
    }
}
/// A rejected operation leaves the coalescer and its sequence unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A bound or identity is invalid.
    InvalidConfig,
    /// The injected clock moved backwards.
    ClockRegressed,
    /// Evidence would exceed the supplied gap budget; retry with a larger budget.
    GapBudget { required: usize, available: usize },
    /// A per-unit sequence cannot be advanced without wrapping.
    SequenceExhausted,
    /// A per-unit gap identifier cannot be allocated without wrapping.
    GapIdExhausted,
}
