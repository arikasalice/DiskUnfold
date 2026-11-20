import type {
  AnalysisJobStatus,
  EventEnvelope,
  JobState,
  ScanControl,
  ScanProblem,
} from "../ipc";

/** Determines whether a status may still transition. */
export function isRunning(state: JobState): boolean {
  return !["done", "partial", "failed", "cancelled"].includes(state);
}

export function canControl(
  command: ScanControl,
  state: JobState | undefined,
): boolean {
  switch (command) {
    case "pause":
      return state === "running";
    case "resume":
      return state === "pause_requested" || state === "paused";
    case "cancel":
      return state !== "cancel_requested";
  }
}

export function controlObserved(
  command: ScanControl,
  state: JobState,
): boolean {
  if (state === "cancel_requested") return true;
  switch (command) {
    case "pause":
      return state === "pause_requested" || state === "paused";
    case "resume":
      return state === "running";
    case "cancel":
      return false;
  }
}

export function controlProblem(
  command: ScanControl,
  outcome: string | undefined,
): ScanProblem {
  if (outcome === "already_terminal") return { kind: "already_ended" };
  return outcome === undefined || outcome === "unsupported"
    ? { kind: "control_unavailable", command }
    : { kind: "control_rejected", command };
}

/** Whether the document is hidden, so polling and frame work can pause. */
export function documentHidden(): boolean {
  return typeof document !== "undefined" && document.hidden;
}

export type ControlView = {
  jobRef: string;
  command?: ScanControl;
  problem?: ScanProblem;
};

export function controlsForJob(
  jobRef: string | undefined,
  status: AnalysisJobStatus | undefined,
  unavailable: boolean,
  control: ControlView | undefined,
) {
  const running =
    jobRef !== undefined &&
    !unavailable &&
    (!status || isRunning(status.state));
  const current = running && control?.jobRef === jobRef ? control : undefined;
  return {
    running,
    control: current?.command,
    controlError: current?.problem,
  };
}

export function isNewLineageEvent(
  event: EventEnvelope,
  lineage: string | undefined,
  sequence: number,
): boolean {
  const body = event.body;
  return (
    event.topic === "results.version_changed" &&
    lineage !== undefined &&
    body !== null &&
    typeof body === "object" &&
    "lineageId" in body &&
    body.lineageId === lineage &&
    "sequence" in body &&
    typeof body.sequence === "number" &&
    Number.isSafeInteger(body.sequence) &&
    body.sequence > sequence
  );
}

/** Returns a value only when it belongs to the given job. */
export function jobValue<T extends { jobRef: string }>(
  value: T | undefined,
  jobRef: string | undefined,
): T | undefined {
  return value && value.jobRef === jobRef ? value : undefined;
}

/** Progress is shown only for the current job while it can still change. */
export function runningValue<T>(
  value: { jobRef: string; value: T } | undefined,
  jobRef: string | undefined,
  status: AnalysisJobStatus | undefined,
): T | undefined {
  return status && isRunning(status.state)
    ? jobValue(value, jobRef)?.value
    : undefined;
}
