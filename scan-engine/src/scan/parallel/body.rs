//! The body of one metadata collection: preparation, workers and the publisher.
use super::*;

pub(super) fn run(
    mut build: BuildHandle,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    control: Control<'_>,
    counts: &mut JobCounts,
    clock: &dyn MonotonicClock,
    options: ScanOptions<'_>,
) -> Result<(Id, bool, usize), BuildError> {
    let Control {
        ids,
        context,
        limit,
        policy,
    } = control;
    let ScanOptions {
        devices,
        diagnostics,
        scan_control,
        correlated_budget,
    } = options;
    let root_count = roots.len();
    let sharing = roots
        .iter()
        .map(|root| record::root_sharing(root.as_ref()))
        .collect();
    let volumes = build.root_volumes();
    let disconnects = disconnect::Disconnects::new(root_count, &volumes);
    let mut reports = volume_reports::Reports::new(volumes);
    build.set_volume_reports(reports.snapshot())?;
    for index in 0..root_count {
        build.add_directory_at(index as u32, Vec::new())?;
    }
    if context.checkpoint() == Checkpoint::Cancel {
        return seal_unstarted(build, ids, root_count, None);
    }
    let prepared = match preparation::prepare(&mut reports, roots, context, &disconnects) {
        Ok(prepared) => prepared,
        Err(JobFlowError::Cancelled) => {
            return seal_unstarted(build, ids, root_count, None);
        }
        Err(_) => return Err(BuildError::Unavailable),
    };
    build.set_volume_reports(reports.snapshot())?;
    let counted_roots =
        preparation::record_starts(&mut build, &prepared.starts, &disconnects, counts, context)?;
    let flows = match acquire_flows(context, limit) {
        Ok(flows) => flows,
        Err(JobFlowError::Cancelled) => {
            return seal_unstarted(build, ids, root_count, Some(&prepared.starts));
        }
        Err(_) => return Err(BuildError::Unavailable),
    };
    let correlated_budget =
        correlated_budget.unwrap_or_else(correlated_budget::CorrelatedBudget::new);
    let shared = scheduler::create_with_budget(
        prepared,
        devices,
        diagnostics,
        disconnects,
        scan_control,
        correlated_budget,
    )?;
    let channel_bytes = size_of::<Event>().saturating_mul(BUFFERED_OBSERVATIONS);
    // The base result channel still drives ordinary scans if this optional evidence pool
    // cannot budget its single inline ring. Correlated selection then falls back before read.
    let _channel_permit = shared.correlated_budget.reserve_channel(channel_bytes);
    let (sender, receiver) = mpsc::sync_channel(BUFFERED_OBSERVATIONS);
    let mut progress = Progress {
        failed_roots: vec![false; root_count],
        root_published: vec![false; root_count],
        counted_roots,
        published: false,
        cadence: PublicationCadence::default(),
        sharing,
    };
    let mut failure = None;
    thread::scope(|scope| {
        let spawned = spawn_workers(scope, flows, &shared, &sender, context, policy)
            .and_then(|()| start_queue(&shared));
        drop(sender);
        if let Err(error) = spawned {
            failure = Some(error);
            shared.abort(context);
            drop(receiver);
            return;
        }
        let services = Services {
            context,
            ids,
            clock,
            abort: &shared.abort,
        };
        if let Err(error) = consume(receiver, &mut progress, &mut build, counts, services) {
            failure = Some(error);
            shared.abort(context);
        }
    });
    // Preserve the peak after workers stop, including failure and cancellation.
    queue::record_ready_peak(&shared, diagnostics);
    if let Some(error) = failure {
        return Err(error);
    }
    if shared.abort.load(Ordering::Acquire) {
        return Err(BuildError::Unavailable);
    }
    let was_cancelled = context.checkpoint() == Checkpoint::Cancel;
    if !was_cancelled && !progress.has_usable_result(&shared.disconnects) {
        return Err(BuildError::Unavailable);
    }
    reports.finish(context, was_cancelled, &shared.disconnects);
    let cancelled = was_cancelled || context.checkpoint() == Checkpoint::Cancel;
    let unprocessed_roots = if cancelled {
        shared
            .queue
            .lock()
            .map_err(|_| BuildError::Unavailable)?
            .root_remaining
            .iter()
            .filter(|&&count| count != 0)
            .count()
    } else {
        0
    };
    let completeness = if cancelled {
        ResultCompleteness::Cancelled
    } else if counts.failed > 0 {
        ResultCompleteness::Missing
    } else {
        ResultCompleteness::Complete
    };
    build.set_volume_reports(reports.snapshot())?;
    let id = ids.next_id();
    build.seal_at(id.clone(), completeness, context.utc_now())?;
    Ok((id, cancelled, unprocessed_roots))
}

/// Seals a collection cancelled before any worker started.
///
/// No root was read, so each root is marked as unread, except one already
/// recorded as a revisit of an earlier root, which is never enumerated, and
/// an unverifiable root, whose start already recorded a gap.
/// `starts` is `None` when the roots were not decided yet.
fn seal_unstarted(
    mut build: BuildHandle,
    ids: &dyn ResultIds,
    root_count: usize,
    starts: Option<&[admission::RootStart]>,
) -> Result<(Id, bool, usize), BuildError> {
    for index in 0..root_count {
        let start = starts.and_then(|starts| starts.get(index)).copied();
        if matches!(
            start,
            Some(admission::RootStart::Revisit | admission::RootStart::Collect(Boundary::Unknown))
        ) {
            continue;
        }
        let root = u32::try_from(index).map_err(|_| BuildError::Unavailable)?;
        build.set_directory_unread(root, &[], UnreadReason::Cancelled)?;
    }
    let id = ids.next_id();
    build.seal(id.clone(), ResultCompleteness::Cancelled)?;
    Ok((id, true, root_count))
}
