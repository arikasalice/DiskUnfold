import type {
  AnalysisClient,
  AnalysisJobProgress,
  EventEnvelope,
  ScanLane,
  ScanLaneStatus,
} from "../ipc";

const MAX_PENDING = 1024;
const TIMEOUT_MS = 10_000;
const INITIAL_RETRY_MS = 1_000;
const MAX_RETRY_MS = 8_000;
type Update = {
  sequence: number;
  terminal: boolean;
  progress?: AnalysisJobProgress;
  laneStatus?: ScanLaneStatus;
};

function count(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

/** Reads a job.progress body; a malformed body yields nothing. */
function progressBody(body: object): AnalysisJobProgress | undefined {
  const value = body as Partial<AnalysisJobProgress>;
  if (
    !count(value.completed) ||
    typeof value.totalKnown !== "boolean" ||
    (value.unit !== "items" && value.unit !== "bytes") ||
    (value.totalKnown && !count(value.total))
  )
    return undefined;
  return value as AnalysisJobProgress;
}

function decimal(value: unknown): value is string {
  return typeof value === "string" && /^\d+$/u.test(value);
}

/** Reads a lane-status body; unknown wait reasons remain valid contract values. */
function laneStatusBody(body: object): ScanLaneStatus | undefined {
  const value = body as Partial<ScanLaneStatus>;
  if (
    !count(value.sequence) ||
    value.sequence < 1 ||
    !Array.isArray(value.lanes) ||
    typeof value.observedAt !== "string" ||
    (value.laneCount !== undefined && !decimal(value.laneCount))
  )
    return undefined;
  const laneIsValid = (candidate: unknown): candidate is ScanLane => {
    if (!candidate || typeof candidate !== "object") return false;
    const lane = candidate as Partial<ScanLane>;
    return (
      count(lane.laneIndex) &&
      Array.isArray(lane.rootIndexes) &&
      lane.rootIndexes.every(count) &&
      (lane.rootCount === undefined || decimal(lane.rootCount)) &&
      count(lane.inFlight) &&
      count(lane.limit) &&
      lane.limit > 0 &&
      typeof lane.waitReason === "string" &&
      typeof lane.waitConfirmed === "boolean" &&
      decimal(lane.remainingDirectories) &&
      decimal(lane.failures)
    );
  };
  return value.lanes.every(laneIsValid) ? (value as ScanLaneStatus) : undefined;
}

function updateBody(topic: string, body: unknown): Update | undefined {
  if (
    !body ||
    typeof body !== "object" ||
    !("sequence" in body) ||
    typeof body.sequence !== "number" ||
    !Number.isSafeInteger(body.sequence) ||
    body.sequence < 1
  )
    return undefined;
  const progress = topic === "job.progress" ? progressBody(body) : undefined;
  const laneStatus =
    topic === "scan.lane_status" ? laneStatusBody(body) : undefined;
  if (topic === "scan.lane_status" && !laneStatus) return undefined;
  return {
    sequence: body.sequence,
    terminal: topic === "job.finished",
    ...(progress ? { progress } : {}),
    ...(laneStatus ? { laneStatus } : {}),
  };
}

/**
 * Maintains one active job's list baseline and orders later job notifications.
 *
 * Progress is delivered without a status read. Lane values use their own
 * sequence, are cleared on resync, and must follow the new baseline. Terminal
 * and stale ordered notifications are discarded.
 */
export function watchJobBaseline(
  client: AnalysisClient,
  jobRef: string,
  changed: () => void,
  failed: (reason: unknown) => void,
  progressed?: (progress: AnalysisJobProgress) => void,
  laneStatusChanged?: (status: ScanLaneStatus) => void,
  laneStatusCleared?: () => void,
) {
  let alive = true;
  let epoch = 0;
  let ready = false;
  let sequence = -1;
  let laneSequence = -1;
  let terminal = false;
  let pending: Update[] = [];
  let timeout: ReturnType<typeof setTimeout> | undefined;
  let retry: ReturnType<typeof setTimeout> | undefined;
  let retryDelay = INITIAL_RETRY_MS;

  function accept(update: Update) {
    if (terminal || update.sequence <= sequence) return false;
    sequence = update.sequence;
    terminal = update.terminal;
    return true;
  }

  function deliver(update: Update) {
    if (update.progress && progressed) progressed(update.progress);
    else changed();
  }

  function restart() {
    const request = ++epoch;
    ready = false;
    pending = [];
    laneSequence = -1;
    laneStatusCleared?.();
    if (timeout !== undefined) clearTimeout(timeout);
    if (retry !== undefined) clearTimeout(retry);
    timeout = setTimeout(() => {
      fail(request, new Error("Job list timed out"));
    }, TIMEOUT_MS);
    void client
      .call("job.list", {})
      .then((rows) => {
        if (!alive || request !== epoch) return;
        const row = rows.find((item) => item.jobRef === jobRef);
        const complete = (cut: number, ended: boolean) => {
          if (!alive || request !== epoch) return;
          if (timeout !== undefined) clearTimeout(timeout);
          timeout = undefined;
          sequence = cut;
          laneSequence = cut;
          terminal = ended;
          pending.sort((left, right) => left.sequence - right.sequence);
          let progress: AnalysisJobProgress | undefined;
          let laneStatus: ScanLaneStatus | undefined;
          for (const update of pending) {
            if (update.laneStatus) {
              if (!terminal && update.sequence > laneSequence) {
                laneSequence = update.sequence;
                sequence = Math.max(sequence, update.sequence);
                laneStatus = update.laneStatus;
              }
            } else if (accept(update) && update.progress)
              progress = update.progress;
          }
          pending = [];
          ready = true;
          retryDelay = INITIAL_RETRY_MS;
          changed();
          if (progress && !terminal) progressed?.(progress);
          if (laneStatus && !terminal) laneStatusChanged?.(laneStatus);
        };
        const confirm = () => {
          void client
            .call("job.get", { jobRef })
            .then((status) => {
              if (!alive || request !== epoch) return;
              if (
                status.terminal &&
                status.jobRef === jobRef &&
                status.sequence >= (row?.sequence ?? 0) &&
                status.terminal.sequence === status.sequence &&
                !pending.some(
                  (update) =>
                    update.terminal && update.sequence > status.sequence,
                )
              )
                complete(status.sequence, true);
              else retry = setTimeout(confirm, INITIAL_RETRY_MS);
            })
            .catch((reason: unknown) => {
              fail(request, reason);
            });
        };
        if (
          (row &&
            ["done", "partial", "failed", "cancelled"].includes(row.state) &&
            !row.terminal) ||
          pending.some(
            (update) =>
              update.terminal &&
              (!row?.terminal || update.sequence > row.sequence),
          )
        )
          confirm();
        else complete(row?.sequence ?? -1, row?.terminal !== undefined);
      })
      .catch((reason: unknown) => {
        fail(request, reason);
      });
  }

  function fail(request: number, reason: unknown) {
    if (!alive || request !== epoch) return;
    epoch += 1;
    if (timeout !== undefined) clearTimeout(timeout);
    if (retry !== undefined) clearTimeout(retry);
    timeout = undefined;
    const delay = retryDelay;
    retryDelay = Math.min(retryDelay * 2, MAX_RETRY_MS);
    retry = setTimeout(restart, delay);
    failed(reason);
  }

  function buffer(update: Update) {
    if (pending.length >= MAX_PENDING) {
      restart();
      return;
    }
    pending.push(update);
  }

  function applyLaneStatus(update: Update) {
    const value = update.laneStatus;
    if (!value || terminal || update.sequence <= laneSequence) return;
    laneSequence = update.sequence;
    sequence = Math.max(sequence, update.sequence);
    laneStatusChanged?.(value);
  }

  function receive(event: EventEnvelope) {
    if (event.topic === "job.resync_required") {
      restart();
      return;
    }
    if (
      event.jobRef !== jobRef ||
      ![
        "job.state_changed",
        "job.progress",
        "job.finished",
        "scan.lane_status",
      ].includes(event.topic)
    )
      return;
    if (event.topic === "scan.lane_status" && terminal) return;
    const update = updateBody(event.topic, event.body);
    if (!update) {
      restart();
      return;
    }
    if (!ready) buffer(update);
    else if (update.laneStatus) applyLaneStatus(update);
    else if (accept(update)) deliver(update);
  }

  const unsubscribe = client.subscribe(receive);
  restart();
  return {
    ready: () => ready,
    sequence: () => sequence,
    dispose: () => {
      alive = false;
      epoch += 1;
      if (timeout !== undefined) clearTimeout(timeout);
      if (retry !== undefined) clearTimeout(retry);
      unsubscribe();
    },
  };
}
