import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AnalysisClient,
  AnalysisJobListRow,
  AnalysisJobProgress,
  AnalysisJobStatus,
  AnalysisJobTerminal,
  EventEnvelope,
} from "../ipc";

/** A control command exposed by the Job Center. */
export type JobCenterCommand = "pause" | "resume" | "cancel";

/** One job's current display data and its event-ordering cursor. */
export type JobCenterCard = {
  row: AnalysisJobListRow;
  progress?: AnalysisJobProgress;
  terminal?: AnalysisJobTerminal;
  sequence: number;
  terminalReceived: boolean;
  order: number;
};

/** A short control result kept on its card until state changes or ten seconds pass. */
export type JobCenterFeedback = {
  id: number;
  command: JobCenterCommand;
  kind: "neutral" | "error";
  reasonKey?: string;
};

/** View of the app-run job list and its current recovery state. */
export type JobCenterSnapshot = {
  cards: JobCenterCard[];
  inProgress: JobCenterCard[];
  recentlyFinished: JobCenterCard[];
  pending: ReadonlyMap<string, JobCenterCommand>;
  feedback: ReadonlyMap<string, JobCenterFeedback>;
  loading: boolean;
  resyncing: boolean;
  error: boolean;
};

type JobUpdate =
  | {
      jobRef: string;
      topic: "job.state_changed";
      sequence: number;
      state: AnalysisJobListRow["state"];
    }
  | { jobRef: string; topic: "job.progress"; progress: AnalysisJobProgress }
  | { jobRef: string; topic: "job.finished"; terminal: AnalysisJobTerminal };

type PendingRequest = {
  command: JobCenterCommand;
  after: number;
  acknowledged?: number;
};

const MAX_BUFFERED_EVENTS = 1024;
const FINISHED_LIMIT = 5;
const FEEDBACK_MS = 10_000;
const terminalStates = new Set(["done", "partial", "failed", "cancelled"]);
const states = new Set([
  "queued",
  "running",
  "pause_requested",
  "paused",
  "cancel_requested",
  ...terminalStates,
]);

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function count(value: unknown): value is number {
  return Number.isSafeInteger(value) && typeof value === "number" && value >= 0;
}

function sequence(value: unknown): value is number {
  return count(value) && value > 0;
}

function isTerminal(state: string): boolean {
  return terminalStates.has(state);
}

function terminalBody(body: unknown): AnalysisJobTerminal | undefined {
  if (!record(body)) return undefined;
  if (
    !sequence(body.sequence) ||
    typeof body.state !== "string" ||
    !isTerminal(body.state) ||
    !count(body.completed) ||
    !count(body.overwritten) ||
    !count(body.failed) ||
    !count(body.skipped) ||
    !count(body.unprocessed) ||
    typeof body.historyRecorded !== "boolean"
  )
    return undefined;
  return body as unknown as AnalysisJobTerminal;
}

function stateUpdate(event: EventEnvelope): JobUpdate | "invalid" {
  const jobRef = event.jobRef;
  if (!jobRef || !record(event.body)) return "invalid";
  const { sequence: seq, state } = event.body;
  if (!sequence(seq) || typeof state !== "string" || !states.has(state))
    return "invalid";
  return {
    jobRef,
    topic: "job.state_changed",
    sequence: seq,
    state: state as AnalysisJobListRow["state"],
  };
}

function progressUpdate(event: EventEnvelope): JobUpdate | "invalid" {
  const jobRef = event.jobRef;
  if (!jobRef || !record(event.body)) return "invalid";
  const body = event.body;
  if (
    !sequence(body.sequence) ||
    !count(body.completed) ||
    typeof body.totalKnown !== "boolean" ||
    (body.totalKnown && !count(body.total)) ||
    (body.unit !== "items" && body.unit !== "bytes") ||
    typeof body.waitReason !== "string"
  )
    return "invalid";
  return {
    jobRef,
    topic: "job.progress",
    progress: body as AnalysisJobProgress,
  };
}

function finishedUpdate(event: EventEnvelope): JobUpdate | "invalid" {
  if (!event.jobRef) return "invalid";
  const terminal = terminalBody(event.body);
  return terminal
    ? { jobRef: event.jobRef, topic: "job.finished", terminal }
    : "invalid";
}

function update(event: EventEnvelope): JobUpdate | "invalid" | undefined {
  switch (event.topic) {
    case "job.state_changed":
      return stateUpdate(event);
    case "job.progress":
      return progressUpdate(event);
    case "job.finished":
      return finishedUpdate(event);
    default:
      return undefined;
  }
}

function targetState(
  command: JobCenterCommand,
  state: string,
  terminal: boolean,
) {
  switch (command) {
    case "pause":
      return state === "paused";
    case "resume":
      return state === "running";
    case "cancel":
      return terminal;
  }
}

function rank(state: string) {
  if (state === "queued") return 1;
  if (state === "paused") return 2;
  return 0;
}

/**
 * Job kinds the user starts and can name. Screens also run short jobs of their
 * own (target probes, folder queries, chart computation); those stay out of the
 * Job Center, its header count and its finish notices.
 */
const USER_TASK_KINDS = new Set(["scan.metadata", "duplicate.check"]);

function userTask(card: JobCenterCard) {
  return USER_TASK_KINDS.has(card.row.kind);
}

/** Maintains the live, session-only task list from its baseline and ordered events. */
export function createJobCenterStore(
  client: AnalysisClient,
  onFinished: (card: JobCenterCard) => void = () => undefined,
) {
  let alive = true;
  let ready = false;
  let loading = false;
  let resyncing = false;
  let error = false;
  let epoch = 0;
  let nextOrder = 0;
  let nextFeedbackId = 0;
  const cards = new Map<string, JobCenterCard>();
  let recentlyFinished: JobCenterCard[] = [];
  const pending = new Map<string, PendingRequest>();
  const feedback = new Map<string, JobCenterFeedback>();
  const feedbackTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const buffered: JobUpdate[] = [];
  const listeners = new Set<(snapshot: JobCenterSnapshot) => void>();
  let unsubscribe: (() => void) | undefined;

  function snapshot(): JobCenterSnapshot {
    const all = [...cards.values()];
    const inProgress = all
      .filter((card) => !isTerminal(card.row.state) && userTask(card))
      .sort(
        (left, right) =>
          rank(left.row.state) - rank(right.row.state) ||
          left.order - right.order,
      );
    return {
      cards: all,
      inProgress,
      recentlyFinished: [...recentlyFinished],
      pending: new Map(
        [...pending].map(([jobRef, request]) => [jobRef, request.command]),
      ),
      feedback: new Map(feedback),
      loading,
      resyncing,
      error,
    };
  }

  function emit() {
    const current = snapshot();
    for (const listener of listeners) listener(current);
  }

  function clearFeedback(jobRef: string) {
    const timer = feedbackTimers.get(jobRef);
    if (timer !== undefined) clearTimeout(timer);
    feedbackTimers.delete(jobRef);
    feedback.delete(jobRef);
  }

  function setFeedback(
    jobRef: string,
    command: JobCenterCommand,
    kind: JobCenterFeedback["kind"],
    reasonKey?: string | null,
  ) {
    clearFeedback(jobRef);
    const id = ++nextFeedbackId;
    feedback.set(jobRef, {
      id,
      command,
      kind,
      ...(reasonKey ? { reasonKey } : {}),
    });
    feedbackTimers.set(
      jobRef,
      setTimeout(() => {
        if (feedback.get(jobRef)?.id !== id) return;
        feedback.delete(jobRef);
        feedbackTimers.delete(jobRef);
        emit();
      }, FEEDBACK_MS),
    );
  }

  function addRecentlyFinished(card: JobCenterCard) {
    if (
      !userTask(card) ||
      recentlyFinished.some((item) => item.row.jobRef === card.row.jobRef)
    )
      return;
    recentlyFinished = [
      card,
      ...recentlyFinished.filter((item) => item.row.jobRef !== card.row.jobRef),
    ].slice(0, FINISHED_LIMIT);
    onFinished(card);
  }

  function updateCard(
    jobRef: string,
    transform: (current: JobCenterCard) => JobCenterCard,
  ) {
    const current = cards.get(jobRef);
    if (!current) return undefined;
    const next = transform(current);
    cards.set(jobRef, next);
    return next;
  }

  function clearMatchingRequest(card: JobCenterCard) {
    const request = pending.get(card.row.jobRef);
    if (
      !request ||
      !targetState(request.command, card.row.state, card.terminalReceived)
    )
      return;
    if (
      card.sequence <= request.after ||
      (request.acknowledged !== undefined &&
        card.sequence < request.acknowledged)
    )
      return;
    pending.delete(card.row.jobRef);
  }

  function apply(change: JobUpdate) {
    const current = cards.get(change.jobRef);
    if (!current || current.terminalReceived) return false;
    const changeSequence =
      change.topic === "job.progress"
        ? change.progress.sequence
        : change.topic === "job.finished"
          ? change.terminal.sequence
          : change.sequence;
    if (changeSequence <= current.sequence) return false;

    if (change.topic === "job.progress") {
      const next = updateCard(change.jobRef, (card) => ({
        ...card,
        sequence: changeSequence,
        progress: change.progress,
        row: {
          ...card.row,
          sequence: changeSequence,
          waitReason: change.progress.waitReason,
        },
      }));
      if (next) clearMatchingRequest(next);
      emit();
      return true;
    }

    if (change.topic === "job.state_changed") {
      const stateChanged = current.row.state !== change.state;
      const next = updateCard(change.jobRef, (card) => ({
        ...card,
        sequence: changeSequence,
        row: { ...card.row, sequence: changeSequence, state: change.state },
      }));
      if (stateChanged) clearFeedback(change.jobRef);
      if (next) clearMatchingRequest(next);
      emit();
      return true;
    }

    const next = updateCard(change.jobRef, (card) => ({
      ...card,
      sequence: changeSequence,
      terminal: change.terminal,
      terminalReceived: true,
      row: {
        ...card.row,
        sequence: changeSequence,
        state: change.terminal.state,
        terminal: change.terminal,
      },
    }));
    if (!next) return false;
    pending.delete(change.jobRef);
    clearFeedback(change.jobRef);
    addRecentlyFinished(next);
    emit();
    return true;
  }

  function bufferChange(change: JobUpdate) {
    if (buffered.length >= MAX_BUFFERED_EVENTS) {
      buffered.length = 0;
      void load(true);
      return;
    }
    buffered.push(change);
  }

  function receive(event: EventEnvelope) {
    if (event.topic === "job.resync_required") {
      void load(true);
      return;
    }
    const change = update(event);
    if (change === undefined) return;
    if (change === "invalid") {
      void load(true);
      return;
    }
    if (loading || !ready) {
      bufferChange(change);
      return;
    }
    if (!cards.has(change.jobRef)) {
      bufferChange(change);
      void load(true);
      return;
    }
    apply(change);
  }

  function validRow(row: AnalysisJobListRow) {
    if (!record(row)) return false;
    const hasFirstRoot = Object.hasOwn(row, "scopeFirstRoot");
    const hasRootCount = Object.hasOwn(row, "scopeRootCount");
    const validScope =
      (!hasFirstRoot && !hasRootCount) ||
      (hasFirstRoot &&
        hasRootCount &&
        typeof row.scopeFirstRoot === "string" &&
        typeof row.scopeRootCount === "number" &&
        Number.isSafeInteger(row.scopeRootCount) &&
        row.scopeRootCount >= 1);
    return (
      typeof row.jobRef === "string" &&
      typeof row.kind === "string" &&
      typeof row.scopeSummary === "string" &&
      validScope &&
      typeof row.state === "string" &&
      states.has(row.state) &&
      count(row.sequence) &&
      Array.isArray(row.supportedControls)
    );
  }

  function confirmTerminal(jobRef: string, markRecent: boolean) {
    void client
      .call("job.get", { jobRef })
      .then((status: AnalysisJobStatus) => {
        const terminal = status.terminal;
        if (!alive || status.jobRef !== jobRef || !terminal) return;
        const current = cards.get(jobRef);
        if (
          !current ||
          current.terminalReceived ||
          status.sequence < current.sequence
        )
          return;
        if (
          terminal.sequence !== status.sequence ||
          terminal.state !== status.state ||
          !isTerminal(status.state)
        )
          return;
        const next = updateCard(jobRef, (card) => ({
          ...card,
          sequence: status.sequence,
          terminal,
          terminalReceived: true,
          row: {
            ...card.row,
            sequence: status.sequence,
            state: status.state,
            terminal,
          },
        }));
        if (!next) return;
        if (markRecent) addRecentlyFinished(next);
        emit();
      })
      .catch(() => undefined);
  }

  function replaceBaseline(rows: AnalysisJobListRow[]) {
    const previous = new Map(cards);
    const nextCards = new Map<string, JobCenterCard>();
    const terminalChecks: { jobRef: string; markRecent: boolean }[] = [];
    rows.forEach((row, index) => {
      const old = previous.get(row.jobRef);
      const markRecent = Boolean(old && !old.terminalReceived);
      const card: JobCenterCard = {
        row,
        ...(old?.progress ? { progress: old.progress } : {}),
        ...(row.terminal
          ? { terminal: row.terminal }
          : old?.terminal
            ? { terminal: old.terminal }
            : {}),
        sequence: row.sequence,
        terminalReceived:
          row.terminal !== undefined || old?.terminalReceived === true,
        order: old?.order ?? nextOrder + index,
      };
      nextCards.set(row.jobRef, card);
      if (isTerminal(row.state) && !card.terminalReceived)
        terminalChecks.push({ jobRef: row.jobRef, markRecent });
      if (row.terminal && markRecent) addRecentlyFinished(card);
    });
    nextOrder += rows.length;
    for (const recent of recentlyFinished) {
      if (!nextCards.has(recent.row.jobRef))
        nextCards.set(recent.row.jobRef, recent);
    }
    cards.clear();
    for (const [jobRef, card] of nextCards) cards.set(jobRef, card);
    return terminalChecks;
  }

  function replayBufferedChanges() {
    const changes = buffered.splice(0);
    const unknownJobs = new Set(
      changes
        .map((change) => change.jobRef)
        .filter((jobRef) => !cards.has(jobRef)),
    );
    for (const change of changes) {
      const current = cards.get(change.jobRef);
      if (
        change.topic === "job.finished" &&
        current?.terminalReceived &&
        current.sequence === change.terminal.sequence
      ) {
        addRecentlyFinished(current);
      } else {
        apply(change);
      }
    }
    return unknownJobs.size > 0;
  }

  async function load(isResync: boolean) {
    if (!alive) return;
    const request = ++epoch;
    loading = true;
    resyncing = isResync && cards.size > 0;
    error = false;
    if (resyncing)
      for (const [jobRef, card] of cards)
        cards.set(jobRef, { ...card, sequence: -1 });
    ready = false;
    emit();
    try {
      const rows = await client.call("job.list", {});
      if (!alive || request !== epoch) return;
      if (!Array.isArray(rows) || rows.some((row) => !validRow(row)))
        throw new Error("Invalid job.list response");

      const terminalChecks = replaceBaseline(rows);
      ready = true;
      loading = false;
      resyncing = false;
      error = false;

      const unknownJobs = replayBufferedChanges();
      for (const check of terminalChecks)
        confirmTerminal(check.jobRef, check.markRecent);
      emit();
      if (unknownJobs) void load(true);
    } catch {
      if (!alive || request !== epoch) return;
      loading = false;
      resyncing = false;
      error = true;
      emit();
    }
  }

  async function control(jobRef: string, command: JobCenterCommand) {
    const card = cards.get(jobRef);
    if (!card || card.terminalReceived || pending.has(jobRef)) return;
    const request: PendingRequest = { command, after: card.sequence };
    pending.set(jobRef, request);
    clearFeedback(jobRef);
    emit();
    try {
      const response = await client.call(`job.${command}`, { jobRef });
      if (!alive || pending.get(jobRef) !== request) return;
      if (response.outcome === "accepted") {
        request.acknowledged = response.sequence;
        const latest = cards.get(jobRef);
        if (
          latest &&
          targetState(command, latest.row.state, latest.terminalReceived) &&
          latest.sequence > request.after &&
          latest.sequence >= response.sequence
        )
          pending.delete(jobRef);
        emit();
        return;
      }
      pending.delete(jobRef);
      const latest = cards.get(jobRef);
      if (latest && response.sequence > latest.sequence) {
        updateCard(jobRef, (current) => ({
          ...current,
          sequence: response.sequence,
          row: {
            ...current.row,
            sequence: response.sequence,
            state: response.state,
          },
        }));
      }
      setFeedback(jobRef, command, "neutral", response.reasonKey);
      if (response.outcome === "already_terminal")
        confirmTerminal(jobRef, true);
      emit();
    } catch {
      if (!alive || pending.get(jobRef) !== request) return;
      pending.delete(jobRef);
      setFeedback(jobRef, command, "error");
      emit();
    }
  }

  return {
    getSnapshot: snapshot,
    subscribe(listener: (value: JobCenterSnapshot) => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    start() {
      if (unsubscribe) return;
      unsubscribe = client.subscribe(receive);
      void load(false);
    },
    retry() {
      if (!loading) void load(cards.size > 0);
    },
    control,
    dispose() {
      if (!alive) return;
      alive = false;
      epoch += 1;
      unsubscribe?.();
      for (const timer of feedbackTimers.values()) clearTimeout(timer);
      feedbackTimers.clear();
      listeners.clear();
    },
  };
}

const emptySnapshot: JobCenterSnapshot = {
  cards: [],
  inProgress: [],
  recentlyFinished: [],
  pending: new Map(),
  feedback: new Map(),
  loading: false,
  resyncing: false,
  error: false,
};

/** Connects the app session to one job-center store and releases its listener on unmount. */
export function useJobCenter(
  client: AnalysisClient | undefined,
  onFinished: (card: JobCenterCard) => void,
) {
  const callback = useRef(onFinished);
  callback.current = onFinished;
  const store = useRef<ReturnType<typeof createJobCenterStore>>(undefined);
  const [value, setValue] = useState(emptySnapshot);

  useEffect(() => {
    if (!client) {
      setValue(emptySnapshot);
      return;
    }
    const current = createJobCenterStore(client, (card) =>
      callback.current(card),
    );
    store.current = current;
    const unsubscribe = current.subscribe(setValue);
    current.start();
    setValue(current.getSnapshot());
    return () => {
      unsubscribe();
      current.dispose();
      if (store.current === current) store.current = undefined;
    };
  }, [client]);

  const retry = useCallback(() => store.current?.retry(), []);
  const control = useCallback(
    (jobRef: string, command: JobCenterCommand) =>
      store.current?.control(jobRef, command),
    [],
  );
  return { ...value, retry, control };
}
