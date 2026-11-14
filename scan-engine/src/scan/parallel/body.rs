//! The body of one metadata collection: preparation, workers and the publisher.
use super::*;

pub(super) fn run(
    mut build: BuildHandle,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    control: Control<'_>,
    counts: &mut JobCounts,
    devices: DevicePlan,
    clock: &dyn MonotonicClock,
    diagnostics: Option<&crate::scan_job::ScanDiagnostics>,
) -> Result<(Id, bool, usize), BuildError> {
    let Control {
        ids,
        context,
        limit,
        policy,
        scan_control,
    } = control;
    let root_count = roots.len();
    let volumes = build.root_volumes();
    let disconnects = disconnect::Disconnects::new(root_count, &volumes);
    let mut reports = volume_reports::Reports::new(volumes);
    build.set_volume_reports(reports.snapshot())?;
    for index in 0..root_count {
        build.add_directory_at(index as u32, Vec::new())?;
    }
    if context.checkpoint() == Checkpoint::Cancel {
        let id = ids.next_id();
        build.seal(id.clone(), ResultCompleteness::Cancelled)?;
        return Ok((id, true, root_count));
    }
    let prepared = match preparation::prepare(&mut reports, roots, context, &disconnects) {
        Ok(prepared) => prepared,
        Err(JobFlowError::Cancelled) => {
            let id = ids.next_id();
            build.seal(id.clone(), ResultCompleteness::Cancelled)?;
            return Ok((id, true, root_count));
        }
        Err(_) => return Err(BuildError::Unavailable),
    };
    build.set_volume_reports(reports.snapshot())?;
    let counted_roots =
        preparation::record_starts(&mut build, &prepared.starts, &disconnects, counts, context)?;
    let flows = match acquire_flows(context, limit) {
        Ok(flows) => flows,
        Err(JobFlowError::Cancelled) => {
            let id = ids.next_id();
            build.seal(id.clone(), ResultCompleteness::Cancelled)?;
            return Ok((id, true, root_count));
        }
        Err(_) => return Err(BuildError::Unavailable),
    };
    let shared = scheduler::create(prepared, devices, diagnostics, disconnects, scan_control)?;
    let (sender, receiver) = mpsc::sync_channel(BUFFERED_OBSERVATIONS);
    let mut progress = Progress {
        failed_roots: vec![false; root_count],
        root_published: vec![false; root_count],
        counted_roots,
        published: false,
        cadence: PublicationCadence::default(),
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
