import { useSyncExternalStore } from "react";
import type { StartupMemoryKind, StartupStatus } from "../../ipc";

/**
 * What the startup checks found and what the user has already seen of it
 * (F15 FR4.HD1.gui-web.DD1, FR6.HD2.gui-web.DD1; design F15 README 1).
 *
 * The one-line notice, the dot beside "Support information" and the startup
 * checks section read the same startup status. A dismissed notice and a
 * reviewed state are remembered by a state key, so a changed state shows
 * again. A state key uses only `[a-z0-9_.:|]`: `check:result` parts, the
 * grade first and the rows sorted by name, joined with `|`, at most
 * {@link STATE_KEY_LIMIT} bytes. The empty string means nothing is
 * remembered. When the current state no longer matches a
 * remembered key, the key is forgotten, so the same state returning later
 * shows again.
 *
 * The keys outlive the run in the user-env settings `startup.notice_dismissed`
 * and `startup.review_seen` (#195): the startup status brings the stored keys,
 * and a connected writer stores each change. Whenever a status arrives whose
 * stored key differs from the one kept here, the kept key is written again, so
 * a forgotten key is cleared on the host too and a write that failed (for
 * example while the database was busy) is tried again. Until then the key
 * holds for this run.
 */
type StartupMemory = {
  status: StartupStatus | undefined;
  dismissed: string;
  seen: string;
};

const empty: StartupMemory = { status: undefined, dismissed: "", seen: "" };
let memory = empty;
const listeners = new Set<() => void>();
type Writer = (kind: StartupMemoryKind, stateKey: string) => void;
let writer: Writer | undefined;

function update(next: Partial<StartupMemory>) {
  memory = { ...memory, ...next };
  for (const listener of listeners) listener();
}

/** The remembered key that matches the current state, or "". */
function kept(current: string, ...remembered: (string | undefined)[]) {
  return current !== "" && remembered.includes(current) ? current : "";
}

export const startupMemory = {
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
  get: () => memory,
  /** Stores each change of a key through `write` until the returned call. */
  connect(write: Writer) {
    writer = write;
    return () => {
      if (writer === write) writer = undefined;
    };
  },
  setStatus(status: StartupStatus) {
    const dismissed = kept(
      noticeKey(status),
      memory.dismissed,
      status.noticeDismissed,
    );
    const seen = kept(reviewKey(status), memory.seen, status.reviewSeen);
    update({ status, dismissed, seen });
    if ((status.noticeDismissed ?? "") !== dismissed) {
      writer?.("notice_dismissed", dismissed);
    }
    if ((status.reviewSeen ?? "") !== seen) writer?.("review_seen", seen);
  },
  dismiss() {
    const dismissed = noticeKey(memory.status);
    update({ dismissed });
    writer?.("notice_dismissed", dismissed);
  },
  /** Marks the current state as reviewed; the dot stays away until it changes. */
  markSeen() {
    const seen = reviewKey(memory.status);
    if (seen === memory.seen) return;
    update({ seen });
    writer?.("review_seen", seen);
  },
  /** For tests: forget everything, as a launch without stored keys would. */
  reset: () => {
    writer = undefined;
    update(empty);
  },
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

/** The longest state key user-env stores. */
export const STATE_KEY_LIMIT = 200;

/**
 * The state key behind the dot: the grade and the rows that did not pass.
 * Rows that would pass {@link STATE_KEY_LIMIT} are left out from the end; the
 * parts are ASCII, so their length is their size in bytes.
 */
export function reviewKey(status: StartupStatus | undefined) {
  if (!status || status.grade === "pass") return "";
  const rows = status.checks
    .filter((row) => row.result !== "pass" && row.result !== "not_applicable")
    .map((row) => `${row.check}:${row.result}`)
    .sort();
  let key = `grade:${status.grade}`;
  for (const row of rows) {
    if (key.length + 1 + row.length > STATE_KEY_LIMIT) break;
    key += `|${row}`;
  }
  return key;
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
