/**
 * The pace at which a screen reflects newer results of a running scan.
 *
 * The analysis summary uses it today. The folder list is meant to share the
 * same pace so the two screens never show numbers from visibly different
 * moments (F02 design review, "auto reflect during a scan", rules 1 and 8).
 * The engine publishes at most once a second, so a faster pace gains nothing.
 */
export const RESULT_REFLECT_INTERVAL_MS = 1000;

/**
 * The pace of a screen whose open parts are read on their own: the slowest
 * of them, and never faster than the shared pace. A screen changes all of
 * its values together, so one slow part sets the pace for all of them.
 */
export function slowestReflectInterval(intervals: readonly number[]): number {
  return Math.max(RESULT_REFLECT_INTERVAL_MS, ...intervals);
}

/**
 * Spaces automatic result reads at most once per interval.
 *
 * `mark` records that a read started now. `remaining` reports how long the
 * next automatic read must wait. `defer` runs one callback when that wait
 * ends; further calls while one is pending are folded into it, so a burst of
 * publications causes a single read of whatever is newest at that time.
 * An interval given as a function is read at each call, for a pace that
 * changes with what the screen has open.
 */
export type ReflectCadence = {
  mark(): void;
  remaining(): number;
  defer(run: () => void): void;
  dispose(): void;
};

/** Creates a cadence; callers dispose it with the owner of their reads. */
export function createReflectCadence(
  interval: number | (() => number) = RESULT_REFLECT_INTERVAL_MS,
): ReflectCadence {
  const span = typeof interval === "function" ? interval : () => interval;
  let last: number | undefined;
  let timer: number | undefined;
  const remaining = () =>
    last === undefined ? 0 : Math.max(0, last + span() - Date.now());
  return {
    mark() {
      last = Date.now();
    },
    remaining,
    defer(run) {
      if (timer !== undefined) return;
      timer = window.setTimeout(() => {
        timer = undefined;
        run();
      }, remaining());
    },
    dispose() {
      if (timer !== undefined) window.clearTimeout(timer);
      timer = undefined;
    },
  };
}
