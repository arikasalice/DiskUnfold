//! Sequential collection on the caller's thread, with the parallel semantics.
//!
//! Entries and unread directory ranges are recorded through the functions the
//! parallel collector uses (FR12.HD1.app-host.DD1, DEC-006). An explicit
//! disconnection closes the directory that reported it; the directories still
//! on the stack were already being read and are read to their end without
//! opening anything, and every enumerated directory left unread is a gap of
//! its own, as in the parallel collector. A cancellation records the entries
//! already returned without opening any directory, and marks every directory
//! it leaves unopened or partly read as unread once.
//!
//! Each directory reports the entries an exclusion rule dropped directly
//! below it once, when its listing ends or stops, as in the parallel collector.
use super::{
    ResultIds,
    admission::{self, Admission, Boundary, Parent, RootStart, Visited},
    issues, record,
};
use du_analysis_index::{
    BuildError, BuildHandle, IngestSink, SharingEvidence, folder_collection::UnreadReason,
};
use du_core_contracts::{
    Id,
    job::{Checkpoint, JobCounts},
    ports::PortError,
    result::ResultCompleteness,
    scan_failures::ScanFailureStage as Stage,
    scan_observation::{AnchoredDirectory, ObservedEntry, TimedObservedEntry},
};
use std::collections::VecDeque;

/// The largest batch an adapter may return; a larger one is a failed read.
const MAX_BATCH: usize = 8192;

/// How a collection ended.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    Finished,
    Cancelled,
    Disconnected,
}

/// What one step of the traversal leaves for the directory stack.
enum Step {
    Continue,
    Push(Frame),
    Pop,
}

struct Frame {
    path: Vec<Vec<u8>>,
    dir: Box<dyn AnchoredDirectory>,
    /// Returned entries that were not yet recorded.
    entries: VecDeque<ObservedEntry>,
    /// Entered directories of the first root batch, recorded and not yet opened.
    recorded_directories: VecDeque<ObservedEntry>,
    boundary: Boundary,
    /// Entries an exclusion rule dropped directly below this directory so far.
    excluded: u64,
}

impl Frame {
    fn new(path: Vec<Vec<u8>>, dir: Box<dyn AnchoredDirectory>, boundary: Boundary) -> Self {
        Self {
            path,
            dir,
            entries: VecDeque::new(),
            recorded_directories: VecDeque::new(),
            boundary,
            excluded: 0,
        }
    }

    fn child_path(&self, entry: &ObservedEntry) -> Vec<Vec<u8>> {
        let mut path = self.path.clone();
        path.push(entry.name.clone());
        path
    }
}

/// Collects one root and seals the result; returns its id and whether the
/// collection ran to its end. A failed first root read without a
/// disconnection leaves no result, as in the parallel collector.
pub(super) fn run(
    sink: &IngestSink,
    scope: Id,
    root: Box<dyn AnchoredDirectory>,
    ids: &dyn ResultIds,
    checkpoint: &mut dyn FnMut() -> Checkpoint,
    include: &mut dyn FnMut(&[Vec<u8>]) -> bool,
    counts: &mut JobCounts,
) -> Result<(Id, bool), BuildError> {
    let mut build = sink.open(scope)?;
    build.add_directory(Vec::new())?;
    let stop = if checkpoint() == Checkpoint::Cancel {
        build.set_directory_unread(0, &[], UnreadReason::Cancelled)?;
        Stop::Cancelled
    } else {
        let mut root = Frame::new(Vec::new(), root, Boundary::Unknown);
        let visited = start_root(&mut build, &mut root, counts)?;
        let mut walk = Walk {
            build: &mut build,
            counts,
            include,
            checkpoint,
            visited,
            closed: false,
            // An unverifiable root was already counted as failed at its start.
            failed: record::FailedRanges::new(vec![root.boundary == Boundary::Unknown]),
            sharing: record::root_sharing(root.dir.as_ref()),
        };
        walk.root(root, ids)?
    };
    let completeness = match stop {
        Stop::Cancelled => ResultCompleteness::Cancelled,
        _ if counts.failed > 0 => ResultCompleteness::Missing,
        _ => ResultCompleteness::Complete,
    };
    let id = ids.next_id();
    build.seal(id.clone(), completeness)?;
    Ok((id, stop != Stop::Cancelled))
}

/// Records the root's case evidence and how it starts, counts the root item
/// once and reserves its identity.
fn start_root(
    build: &mut BuildHandle,
    root: &mut Frame,
    counts: &mut JobCounts,
) -> Result<Visited, BuildError> {
    if let Some(evidence) = root.dir.observe_directory_case(&|| None) {
        build.set_directory_case_evidence(0, &[], evidence)?;
    }
    let (visited, starts) = Visited::reserve_roots(vec![root.dir.observed_identity()]);
    if let Some(&RootStart::Collect(boundary)) = starts.first() {
        root.boundary = boundary;
    }
    admission::record_root_start(build, 0, RootStart::Collect(root.boundary), counts, None)?;
    Ok(visited)
}

struct Walk<'a> {
    build: &'a mut BuildHandle,
    counts: &'a mut JobCounts,
    include: &'a mut dyn FnMut(&[Vec<u8>]) -> bool,
    checkpoint: &'a mut dyn FnMut() -> Checkpoint,
    visited: Visited,
    /// The volume reported an explicit disconnection; nothing more is opened.
    closed: bool,
    /// The ranges that failed, so that each one is counted once.
    failed: record::FailedRanges,
    /// The sharing evidence of the root's volume, for every file.
    sharing: SharingEvidence,
}

impl Walk<'_> {
    /// Records the first root batch before opening any of its directories.
    fn root(&mut self, mut root: Frame, ids: &dyn ResultIds) -> Result<Stop, BuildError> {
        let batch = match root.dir.next_batch() {
            Ok(batch) if batch.len() <= MAX_BATCH => batch,
            failure => {
                let reason = failure_reason(&failure);
                self.fail(Vec::new(), Stage::Enumerate, reason)?;
                // Only explicit disconnection evidence keeps a result.
                return if reason == issues::DISCONNECTED {
                    Ok(Stop::Disconnected)
                } else {
                    Err(BuildError::Unavailable)
                };
            }
        };
        if batch.is_empty() {
            return Ok(Stop::Finished);
        }
        let disconnected = batch.iter().any(issues::disconnected_entry);
        root.entries = batch.into();
        if disconnected {
            self.close(&mut root)?;
            return Ok(Stop::Disconnected);
        }
        while let Some(entry) = root.entries.pop_front() {
            if (self.checkpoint)() == Checkpoint::Cancel {
                root.entries.push_front(entry);
                return self.cancel(vec![root]);
            }
            let path = root.child_path(&entry);
            let (row, entry) = self.record(&mut root, entry, path, true)?;
            if row == Admission::Enter {
                root.recorded_directories.push_back(entry);
            }
        }
        self.build.publish_partial(ids.next_id())?;
        self.walk(root)
    }

    fn walk(&mut self, root: Frame) -> Result<Stop, BuildError> {
        let mut stack = vec![root];
        while let Some(frame) = stack.last_mut() {
            if (self.checkpoint)() == Checkpoint::Cancel {
                return self.cancel(stack);
            }
            match self.step(frame)? {
                Step::Continue => {}
                Step::Push(child) => stack.push(child),
                Step::Pop => {
                    stack.pop();
                }
            }
        }
        Ok(if self.closed {
            Stop::Disconnected
        } else {
            Stop::Finished
        })
    }

    fn step(&mut self, frame: &mut Frame) -> Result<Step, BuildError> {
        if let Some(entry) = frame.recorded_directories.pop_front() {
            let path = frame.child_path(&entry);
            return self.open(frame, &entry, path);
        }
        if let Some(entry) = frame.entries.pop_front() {
            let path = frame.child_path(&entry);
            // After a disconnection no directory is entered, so none is reserved.
            let reserve = !self.closed;
            let (row, entry) = self.record(frame, entry, path.clone(), reserve)?;
            if row == Admission::Enter {
                return self.open(frame, &entry, path);
            }
            return Ok(Step::Continue);
        }
        match frame.dir.next_batch() {
            Ok(batch) if batch.is_empty() => {
                self.report_excluded(frame, true)?;
                Ok(Step::Pop)
            }
            Ok(batch) if batch.len() <= MAX_BATCH => {
                let disconnected = batch.iter().any(issues::disconnected_entry);
                frame.entries = batch.into();
                if disconnected {
                    self.close(frame)?;
                    return Ok(Step::Pop);
                }
                Ok(Step::Continue)
            }
            failure => {
                let reason = failure_reason(&failure);
                self.fail(frame.path.clone(), Stage::Enumerate, reason)?;
                self.report_excluded(frame, false)?;
                self.closed |= reason == issues::DISCONNECTED;
                Ok(Step::Pop)
            }
        }
    }

    /// Opens an entered directory relative to its parent's handle.
    ///
    /// After a disconnection the directory is not opened and is a gap of its
    /// own, whether or not another collector would have reached it already.
    fn open(
        &mut self,
        parent: &Frame,
        entry: &ObservedEntry,
        path: Vec<Vec<u8>>,
    ) -> Result<Step, BuildError> {
        if self.closed {
            self.fail(path, Stage::Enumerate, issues::DISCONNECTED)?;
            return Ok(Step::Continue);
        }
        match parent.dir.open_child(entry) {
            Ok(dir) => {
                if (self.checkpoint)() != Checkpoint::Cancel
                    && let Some(evidence) = dir.observe_directory_case(&|| None)
                {
                    self.build.set_directory_case_evidence(0, &path, evidence)?;
                }
                Ok(Step::Push(Frame::new(path, dir, Boundary::child(entry))))
            }
            Err(error) => {
                let reason = issues::reason(&error);
                self.fail(path, Stage::OpenDirectory, reason)?;
                self.closed |= reason == issues::DISCONNECTED;
                Ok(Step::Continue)
            }
        }
    }

    /// Applies the admission table to one entry and records it unless excluded.
    /// An excluded entry is counted in `frame`.
    ///
    /// `reserve` is false for an entry that will not be opened, so that no
    /// directory identity is reserved without being entered.
    fn record(
        &mut self,
        frame: &mut Frame,
        entry: ObservedEntry,
        path: Vec<Vec<u8>>,
        reserve: bool,
    ) -> Result<(Admission, ObservedEntry), BuildError> {
        let excluded = !(self.include)(&path);
        let parent = Parent {
            root: 0,
            boundary: frame.boundary,
        };
        let row = if reserve {
            admission::decide(excluded, &entry, parent, &self.visited)
        } else {
            admission::preview(excluded, &entry, parent, &self.visited)
        };
        let timed = TimedObservedEntry::from(entry);
        if row == Admission::Excluded {
            frame.excluded = frame.excluded.saturating_add(1);
        } else {
            let item = (0, path, &timed);
            record::record_item(self.build, self.counts, &|| None, item, row, self.sharing)?;
        }
        Ok((row, timed.entry))
    }

    /// Reports the entries excluded directly below `frame` once, when its
    /// listing ended (`complete`) or stopped; nothing is recorded for none.
    fn report_excluded(&mut self, frame: &Frame, complete: bool) -> Result<(), BuildError> {
        match record::excluded_children(frame.excluded, complete) {
            Some((count, complete)) => {
                self.build
                    .set_excluded_children(0, &frame.path, count, complete)
            }
            None => Ok(()),
        }
    }

    /// Records an unread range; the directory stays counted once, as failed.
    fn fail(&mut self, path: Vec<Vec<u8>>, stage: Stage, reason: &str) -> Result<(), BuildError> {
        let counted = self.failed.counted(0, &path);
        let range = (0, path);
        record::record_failure(
            self.build,
            self.counts,
            range,
            (stage, reason),
            (None, None),
            counted,
        )
    }

    /// Records the returned entries of a cancelled directory without opening
    /// any of them, and marks it and every entered directory among its
    /// entries as unread.
    fn drain(&mut self, mut frame: Frame) -> Result<(), BuildError> {
        while let Some(entry) = frame.entries.pop_front() {
            let path = frame.child_path(&entry);
            let (row, entry) = self.record(&mut frame, entry, path, false)?;
            if row == Admission::Enter {
                self.unread(frame.child_path(&entry))?;
            }
        }
        while let Some(entry) = frame.recorded_directories.pop_front() {
            self.unread(frame.child_path(&entry))?;
        }
        self.report_excluded(&frame, false)?;
        // An unverifiable root already has a gap from its start; a gap and a
        // mark are never left on one directory.
        if frame.boundary == Boundary::Unknown {
            return Ok(());
        }
        self.unread(frame.path)
    }

    fn unread(&mut self, path: Vec<Vec<u8>>) -> Result<(), BuildError> {
        self.failed.unread(0, &path);
        self.build
            .set_directory_unread(0, &path, UnreadReason::Cancelled)
    }

    fn cancel(&mut self, stack: Vec<Frame>) -> Result<Stop, BuildError> {
        for frame in stack.into_iter().rev() {
            self.drain(frame)?;
        }
        Ok(Stop::Cancelled)
    }

    /// Closes the directory whose own read reported a disconnection.
    ///
    /// Its returned entries are recorded without opening any of them; the
    /// directory and every entered directory among them is a gap of its own.
    fn close(&mut self, frame: &mut Frame) -> Result<(), BuildError> {
        self.closed = true;
        while let Some(entry) = frame.recorded_directories.pop_front() {
            self.fail(
                frame.child_path(&entry),
                Stage::Enumerate,
                issues::DISCONNECTED,
            )?;
        }
        while let Some(entry) = frame.entries.pop_front() {
            let path = frame.child_path(&entry);
            let (row, _) = self.record(frame, entry, path.clone(), false)?;
            if row == Admission::Enter {
                self.fail(path, Stage::Enumerate, issues::DISCONNECTED)?;
            }
        }
        self.report_excluded(frame, false)?;
        self.fail(frame.path.clone(), Stage::Enumerate, issues::DISCONNECTED)
    }
}

fn failure_reason(failure: &Result<Vec<ObservedEntry>, PortError>) -> &'static str {
    match failure {
        Err(error) => issues::reason(error),
        Ok(_) => "scan.unclassified",
    }
}
