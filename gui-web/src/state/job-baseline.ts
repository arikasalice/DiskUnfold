import type { AnalysisClient, EventEnvelope } from "../ipc";

const MAX_PENDING = 1024;
const TIMEOUT_MS = 10_000;
const INITIAL_RETRY_MS = 1_000;
const MAX_RETRY_MS = 8_000;
type Update = { sequence: number; terminal: boolean };

/** Maintains one active job's list baseline and orders later job notifications. */
export function watchJobBaseline(
  client: AnalysisClient,
  jobRef: string,
  changed: () => void,
  failed: (reason: unknown) => void,
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
          for (const update of pending) accept(update);
          pending = [];
          ready = true;
          retryDelay = INITIAL_RETRY_MS;
          changed();
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
    const update = {
      sequence: body.sequence,
      terminal: event.topic === "job.finished",
    };
    if (!ready) {
      if (pending.length >= MAX_PENDING) {
        restart();
        return;
      }
      pending.push(update);
    } else if (accept(update)) changed();
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
