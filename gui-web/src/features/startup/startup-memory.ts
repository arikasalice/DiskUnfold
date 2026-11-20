import { useSyncExternalStore } from "react";
import type { StartupStatus } from "../../ipc";

/**
 * What the startup checks found and what the user has already seen of it
 * (F15 FR4.HD1.gui-web.DD1, FR6.HD2.gui-web.DD1; design F15 README 1).
 *
 * The one-line notice, the dot beside "Support information" and the startup
 * checks section read the same startup status. A dismissed notice and a
 * reviewed state are remembered by a state key, so a changed state shows
 * again. A state key uses only `[a-z0-9_.:|]`: `check:result` parts, the
 * grade first and the rows sorted by name, joined with `|`. The empty string
 * means nothing is remembered. When the current state no longer matches a
 * remembered key, the key is forgotten, so the same state returning later
 * shows again. These keys are kept for this run; they move to the user-env
 * settings `startup.notice_dismissed` and `startup.review_seen` once the host
 * can store them.
 */
type StartupMemory = {
  status: StartupStatus | undefined;
  dismissed: string;
  seen: string;
};

const empty: StartupMemory = { status: undefined, dismissed: "", seen: "" };
let memory = empty;
const listeners = new Set<() => void>();

function update(next: Partial<StartupMemory>) {
  memory = { ...memory, ...next };
  for (const listener of listeners) listener();
}

export const startupMemory = {
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
  get: () => memory,
  setStatus: (status: StartupStatus) =>
    update({
      status,
      dismissed: memory.dismissed === noticeKey(status) ? memory.dismissed : "",
      seen: memory.seen === reviewKey(status) ? memory.seen : "",
    }),
  dismiss: () => update({ dismissed: noticeKey(memory.status) }),
  /** Marks the current state as reviewed; the dot stays away until it changes. */
  markSeen: () => update({ seen: reviewKey(memory.status) }),
  /** For tests: forget everything, as a launch without stored keys would. */
  reset: () => update(empty),
};

export function useStartupMemory() {
  return useSyncExternalStore(startupMemory.subscribe, startupMemory.get);
}

/**
 * The state key of the one-line notice, or "" when nothing in the app can act
 * on the state. Today only an unconfirmed Full Disk Access on macOS has an
 * action (open Settings); a grade alone never shows a notice.
 */
export function noticeKey(status: StartupStatus | undefined) {
  return status?.analysisAllowed && status.access === "unknown"
    ? "full_disk_access:unknown"
    : "";
}

/** The state key behind the dot: the grade and the rows that did not pass. */
export function reviewKey(status: StartupStatus | undefined) {
  if (!status || status.grade === "pass") return "";
  const rows = status.checks
    .filter((row) => row.result !== "pass" && row.result !== "not_applicable")
    .map((row) => `${row.check}:${row.result}`)
    .sort();
  return [`grade:${status.grade}`, ...rows].join("|");
}

/** True while the one-line notice should show. */
export function showsNotice(state: StartupMemory) {
  const key = noticeKey(state.status);
  return key !== "" && key !== state.dismissed;
}

/** True while the dot beside "Support information" should show. */
export function needsReview(state: StartupMemory) {
  const key = reviewKey(state.status);
  return key !== "" && key !== state.seen;
}
