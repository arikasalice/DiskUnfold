import type { AppLifecycleChanged, EventEnvelope } from "../../ipc";

/** Where the closing screen is in the host's shutdown steps (F15 IF-07). */
export type ClosingPhase = "access_closed" | "draining" | "closed";

/** The part of an `app.lifecycle_changed` event the closing guard acts on. */
export type ClosingSignal =
  | { kind: "confirm"; changeCount: number; sequence: number }
  | {
      kind: "closing";
      phase: "access_closed" | "draining";
      remaining: number;
      sequence: number;
    }
  | { kind: "closed"; sequence: number }
  | { kind: "ignore" };

function count(value: unknown): number | undefined {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    ? value
    : undefined;
}

/**
 * Reads an IF-07 lifecycle event. An unknown shape or state is ignored: the GUI
 * never infers a closing state from data it cannot read.
 */
export function readClosingSignal(event: EventEnvelope): ClosingSignal {
  if (event.topic !== "app.lifecycle_changed") return { kind: "ignore" };
  const body = event.body as Partial<AppLifecycleChanged> | null;
  if (body === null || typeof body !== "object") return { kind: "ignore" };
  const sequence = count(body.stateSequence);
  if (sequence === undefined) return { kind: "ignore" };
  if (body.state === "running") {
    const changes = count(body.inProgressChangeCount);
    return body.confirmationRequired === true && changes !== undefined
      ? { kind: "confirm", changeCount: changes, sequence }
      : { kind: "ignore" };
  }
  if (body.state === "access_closed" || body.state === "draining") {
    return {
      kind: "closing",
      phase: body.state,
      remaining: count(body.remainingJobCount) ?? 0,
      sequence,
    };
  }
  return body.state === "closed"
    ? { kind: "closed", sequence }
    : { kind: "ignore" };
}
