import type {
  AnalysisClient,
  AnalysisJobProgress,
  EventEnvelope,
} from "../ipc";

const MAX_PENDING = 1024;
const TIMEOUT_MS = 10_000;
const INITIAL_RETRY_MS = 1_000;
const MAX_RETRY_MS = 8_000;
type Update = {
  sequence: number;
  terminal: boolean;
  progress?: AnalysisJobProgress;
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

/**
 * Maintains one active job's list baseline and orders later job notifications.
 *
 * A well-formed job.progress body goes to `progressed` and does not ask for a
 * status read. Other ordered notifications, including a malformed progress
 * body, call `changed`. Values at or below the delivered sequence, after the
 * terminal event, or held across a resynchronization at or below the new
 * baseline are discarded.
 */
export function watchJobBaseline(
  client: AnalysisClient,
  jobRef: string,
  changed: () => void,
  failed: (reason: unknown) => void,
  progressed?: (progress: AnalysisJobProgress) => void,
) {
  let alive = true;
  let epoch = 0;
  let ready = false;
  let sequence = -1;
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
          terminal = ended;
          pending.sort((left, right) => left.sequence - right.sequence);
          let progress: AnalysisJobProgress | undefined;
          for (const update of pending)
            if (accept(update) && update.progress) progress = update.progress;
          pending = [];
          ready = true;
          retryDelay = INITIAL_RETRY_MS;
          changed();
          if (progress && !terminal) progressed?.(progress);
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

  function receive(event: EventEnvelope) {
    if (event.topic === "job.resync_required") {
      restart();
      return;
    }
    if (
      event.jobRef !== jobRef ||
      !["job.state_changed", "job.progress", "job.finished"].includes(
        event.topic,
      )
    )
      return;
    const body = event.body;
    if (
      !body ||
      typeof body !== "object" ||
      !("sequence" in body) ||
      typeof body.sequence !== "number" ||
      !Number.isSafeInteger(body.sequence) ||
      body.sequence < 1
    ) {
      restart();
      return;
    }
    const progress =
      event.topic === "job.progress" ? progressBody(body) : undefined;
    const update: Update = {
      sequence: body.sequence,
      terminal: event.topic === "job.finished",
      ...(progress ? { progress } : {}),
    };
    if (!ready) {
      if (pending.length >= MAX_PENDING) {
        restart();
        return;
      }
      pending.push(update);
    } else if (accept(update)) deliver(update);
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
