use super::*;

pub(super) fn worker_loop(
    shared: &Shared,
    seat: usize,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
    flow: &dyn JobFlow,
) -> bool {
    while context.checkpoint() != Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        if flow.safe_point() == Checkpoint::Cancel {
            break;
        }
        let Some(Claim {
            frame,
            disk_seat,
            mut turn,
        }) = dispatch::take(shared, context, seat, flow)
        else {
            break;
        };
        let _device_seat = match acquire_device_seat(disk_seat, shared, context, turn.lane) {
            Ok(seat) => seat,
            Err(du_core_contracts::job::JobFlowError::Busy) => {
                requeue(shared, &mut turn, frame, None, context);
                continue;
            }
            Err(du_core_contracts::job::JobFlowError::Cancelled) => {
                if !cancel_drain::frame(frame, shared, sender, policy) {
                    return false;
                }
                continue;
            }
            Err(du_core_contracts::job::JobFlowError::Closed) => return true,
            Err(_) => return false,
        };
        if !process(frame, shared, sender, context, policy, &mut turn) {
            return false;
        }
        drop(_device_seat);
    }
    if context.checkpoint() == Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        return cancel_drain::drain(shared, seat, sender, context, policy);
    }
    true
}

fn acquire_device_seat(
    disk: Option<du_core_contracts::job::DiskSeat>,
    shared: &Shared,
    context: &dyn JobContext,
    lane: usize,
) -> Result<Option<du_core_contracts::job::DeviceSeat>, du_core_contracts::job::JobFlowError> {
    let Some(disk) = disk else {
        return Ok(None);
    };
    if let Some(seat) = context.try_device_seat(&disk)? {
        return Ok(Some(seat));
    }
    let _binding = lane_meter::bind(&shared.meters, lane);
    lane_meter::device_wait(|| context.wait_device_seat(&disk)).map(Some)
}
