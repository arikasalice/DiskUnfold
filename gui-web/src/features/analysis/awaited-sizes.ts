import { useState } from "react";
import type { SpaceTotal } from "../../ipc";
import type { SizeWait } from "./space-format";

/**
 * How many results in a row may come without a logical and an allocated size
 * before the two read as unavailable: the scan publishes, but reads no file
 * that has a size (a scope of empty folders, for instance).
 */
export const AWAITED_RESULT_LIMIT = 3;

/** Whether a result has neither a logical nor an allocated size. */
export function lacksReadSizes(total: SpaceTotal): boolean {
  return total.sizes.every(
    ({ size }) => size.basis === "unique_physical" || size.state === "unknown",
  );
}

/** The run of results without a read size, up to the result shown last. */
type Run = { scan: string | undefined; version: string; count: number };

function advance(
  run: Run | undefined,
  total: SpaceTotal,
  scan: string | undefined,
): Run {
  const earlier = run && run.scan === scan ? run.count : 0;
  return {
    scan,
    version: total.resultVersion,
    count: lacksReadSizes(total) ? earlier + 1 : 0,
  };
}

/**
 * Decides by the results of the shown scan whether its sizes are still
 * awaited, given how the scan stands (`wait`; nothing once it ended).
 *
 * A size is awaited only while the result has neither a logical nor an
 * allocated size: a value on one of them means files are being read, so the
 * other is not merely late. Each distinct result version the summary shows
 * for `scan` counts once, starting with the first; from the
 * {@link AWAITED_RESULT_LIMIT}th in a row without the two sizes on, they are
 * no longer awaited. A result with a size ends the run, and another scan
 * starts a new one. The run is state of the summary that shows the result.
 */
export function useSizeWait(
  total: SpaceTotal,
  scan: string | undefined,
  wait: SizeWait | undefined,
): SizeWait | undefined {
  const [run, setRun] = useState(() => advance(undefined, total, scan));
  let shown = run;
  if (run.scan !== scan || run.version !== total.resultVersion) {
    shown = advance(run, total, scan);
    setRun(shown);
  }
  return shown.count > 0 && shown.count < AWAITED_RESULT_LIMIT
    ? wait
    : undefined;
}
