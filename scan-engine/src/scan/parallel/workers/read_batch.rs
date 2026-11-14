//! Read one bounded batch under the selected metadata filter policy.
//!
//! The frozen exclusion rules are the first admission row. A name they cannot
//! decide is withheld from the adapter and returned in `undecided`.
use super::*;

pub(super) enum ReadFailure {
    Filter,
    Observation(PortError),
}

pub(super) fn next_entries(
    frame: &mut Frame,
    context: &dyn JobContext,
    policy: Policy<'_>,
    undecided: &mut Vec<Vec<u8>>,
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
            // Stops this enumeration; the frame is dropped as cancelled.
            du_core_contracts::scan_observation::FilterDecision::Undecidable
        } else {
            admission::exclusion(
                include(frame.root_index, &frame.path, name),
                name,
                undecided,
            )
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
