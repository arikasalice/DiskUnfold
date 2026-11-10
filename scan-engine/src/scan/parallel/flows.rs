//! Bounded scan flow registration.
use super::*;

pub(super) fn acquire_flows(
    context: &dyn JobContext,
    limit: usize,
) -> Result<Vec<Box<dyn JobFlow>>, JobFlowError> {
    if limit == 0 {
        return Err(JobFlowError::UnsupportedResource);
    }
    let first = loop {
        if context.checkpoint() == Checkpoint::Cancel {
            return Err(JobFlowError::Cancelled);
        }
        match context.try_start_flow() {
            Ok(flow) => break flow,
            Err(JobFlowError::Busy) => thread::sleep(Duration::from_millis(25)),
            Err(error) => return Err(error),
        }
    };
    let mut flows = Vec::new();
    flows.push(first);
    for _ in 1..limit.min(MAX_WORKERS) {
        match context.try_start_flow() {
            Ok(flow) => flows.push(flow),
            Err(JobFlowError::Busy) => break,
            Err(_) => break,
        }
    }
    Ok(flows)
}
