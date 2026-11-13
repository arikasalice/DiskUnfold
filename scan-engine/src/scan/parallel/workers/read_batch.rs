//! Read one bounded batch under the selected metadata filter policy.
use super::*;

pub(super) enum ReadFailure {
    Filter,
    Observation(PortError),
}

pub(super) fn next_entries(
    frame: &mut Frame,
    context: &dyn JobContext,
    policy: Policy<'_>,
) -> Result<Option<Vec<TimedObservedEntry>>, ReadFailure> {
    let Policy::BeforeMetadata(include) = policy else {
        return frame
            .directory
            .next_batch_timed(&|| context.utc_now())
            .map(Some)
            .map_err(ReadFailure::Observation);
    };
    let mut decide = |name: &[u8]| {
        if context.checkpoint() == Checkpoint::Cancel {
            du_core_contracts::scan_observation::FilterDecision::Undecidable
        } else {
            include(frame.root_index, &frame.path, name)
        }
    };
    let batch = frame
        .directory
        .next_filtered_timed(&mut decide, &|| context.utc_now())
        .map_err(|error| {
            if error.kind == PortErrorKind::Unsupported {
                ReadFailure::Filter
            } else {
                ReadFailure::Observation(error)
            }
        })?;
    match batch {
        du_core_contracts::scan_observation::TimedFilteredBatch::Entries(entries)
            if !entries.is_empty() =>
        {
            Ok(Some(entries))
        }
        du_core_contracts::scan_observation::TimedFilteredBatch::Progress => Ok(None),
        du_core_contracts::scan_observation::TimedFilteredBatch::End => Ok(Some(Vec::new())),
        _ => Err(ReadFailure::Filter),
    }
}
