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
 * Spaces automatic result reads at most once per interval.
 *
 * `mark` records that a read started now. `remaining` reports how long the
 * next automatic read must wait. `defer` runs one callback when that wait
 * ends; further calls while one is pending are folded into it, so a burst of
 * publications causes a single read of whatever is newest at that time.
 */
export type ReflectCadence = {
  mark(): void;
  remaining(): number;
  defer(run: () => void): void;
  dispose(): void;
};

/** Creates a cadence; callers dispose it with the owner of their reads. */
export function createReflectCadence(
  interval: number = RESULT_REFLECT_INTERVAL_MS,
): ReflectCadence {
  let last: number | undefined;
  let timer: number | undefined;
  const remaining = () =>
    last === undefined ? 0 : Math.max(0, last + interval - Date.now());
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
