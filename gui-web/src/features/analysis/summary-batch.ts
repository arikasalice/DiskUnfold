import { useLayoutEffect, useRef, useState } from "react";
import { AnalysisError } from "../../ipc";
import type { AnalysisClient, SpaceTotal, VolumeGapResponse } from "../../ipc";
import {
  createReflectCadence,
  RESULT_REFLECT_INTERVAL_MS,
  slowestReflectInterval,
} from "../../state/result-cadence";
import { lookupProblem } from "./volume-format";
import type { Message } from "./volume-format";

/**
 * The shortest spacing between two reads of the whole-volume comparison, and
 * so between two summary changes while it is open (F02 design review of
 * PR #69). The comparison is read on its own and costs more to read.
 */
export const VOLUME_COMPARE_REREAD_MS = 2000;

/**
 * How many reads of one open part may fail in a row, while the scan goes on,
 * before the part shows the problem instead of saying it is updating.
 */
export const VOLUME_READ_FAILURE_LIMIT = 3;

/** The scanned folder to explain, or the volume whose scanned folders to compare. */
export type VolumeTarget = { root: number } | { volumeId: string };

export function isWholeVolume(target: VolumeTarget | undefined): boolean {
  return target !== undefined && "volumeId" in target;
}

/**
 * The pace of the summary while `target` is open: one second for the
 * explanation, two for the whole-volume comparison. It is also how long the
 * summary waits for that part to read the result it is about to show. After
 * that the summary changes alone, so a slow read never makes the scan look
 * stopped; waiting less than the pace would only show "updating" more often
 * (F02 design review of PR #69).
 */
export function volumePartPace(target: VolumeTarget): number {
  return isWholeVolume(target)
    ? VOLUME_COMPARE_REREAD_MS
    : RESULT_REFLECT_INTERVAL_MS;
}

/** What the volume part of the summary shows. */
export type VolumeView =
  /** Not open. */
  | { state: "closed" }
  /** Opened, and nothing read for it yet. */
  | { state: "reading" }
  /**
   * The summary moved to a result this part has not read yet. A read of it
   * is in flight or due with the next result.
   */
  | { state: "updating"; volumeId: string | null }
  /**
   * Reading `version` failed and no further read is due. `framed` is whether
   * the part showed values before, and so keeps their frame.
   */
  | {
      state: "failed";
      version: string;
      problem: Message;
      framed: boolean;
      volumeId: string | null;
    }
  /** What was read for `version`, the result the summary shows. */
  | { state: "shown"; version: string; response: VolumeGapResponse };

/** Everything the summary shows; all of it comes from `total.resultVersion`. */
export type SummaryView = {
  total: SpaceTotal | undefined;
  target: VolumeTarget | undefined;
  volume: VolumeView;
};

/** The volume part's view and its controls. */
export type VolumePart = {
  target: VolumeTarget | undefined;
  view: VolumeView;
  open(target: VolumeTarget): void;
  close(): void;
  /** Reads the newest result again after a failure. */
  retry(): void;
};

const CLOSED: VolumeView = { state: "closed" };
const READING: VolumeView = { state: "reading" };
const MISMATCH: Message = [
  "ui.analysis.volume_response_mismatch",
  "The response does not match this scan result.",
];

/** A finished volume read; `replaced` when a newer result refused it. */
type Finished = {
  key: string;
  version: string;
  response?: VolumeGapResponse;
  problem?: Message;
  replaced: boolean;
};

export type SummaryBatch = {
  /**
   * The newest totals read for the scan. `live` is whether the scan will
   * publish a newer result: it is running and not paused.
   */
  offer(total: SpaceTotal | undefined, live: boolean): void;
  /** Holds every value while the user selects text in the summary. */
  hold(on: boolean): void;
  open(target: VolumeTarget): void;
  close(): void;
  retry(): void;
  dispose(): void;
};

/**
 * Changes the summary's totals and its open volume part together, so the
 * summary never shows values from two results.
 *
 * With the part closed, offered totals are shown at once. With it open, a
 * newer total starts a batch: the part reads the batch's result and both
 * change in one view when the read ends. A batch starts at most once per
 * `volumePartPace` of the open part and waits as long at most; after that, or
 * when the read fails, the summary changes alone and the part is "updating".
 * Reads never overlap. A newer total offered during a batch does not restart
 * it; the next batch takes the newest total.
 *
 * The part is "updating" only while a read is in flight or due. A failed
 * read waits for the next result while the scan is live, up to
 * `VOLUME_READ_FAILURE_LIMIT` failures in a row; a refusal because the result
 * was replaced is not counted, since the result replacing it is on its way.
 * Otherwise the part shows the problem until a read succeeds: the batch of
 * the next result reads again, and so does `retry`.
 */
export function createSummaryBatch(
  client: AnalysisClient,
  show: (view: SummaryView) => void,
): SummaryBatch {
  let alive = true;
  let incoming: SpaceTotal | undefined;
  let live = false;
  let holding = false;
  let view: SummaryView = {
    total: undefined,
    target: undefined,
    volume: CLOSED,
  };
  let key: string | undefined;
  let batch: { total: SpaceTotal; started: number } | undefined;
  let deadline: number | undefined;
  let reading = false;
  let finished: Finished | undefined;
  let failures = 0;
  let sequence = 0;
  const cadence = createReflectCadence(() =>
    slowestReflectInterval(view.target ? [volumePartPace(view.target)] : []),
  );

  function commit(next: SummaryView) {
    if (next.total && next.total.resultVersion !== view.total?.resultVersion)
      cadence.mark();
    view = next;
    show(view);
  }

  function endBatch() {
    batch = undefined;
    if (deadline !== undefined) window.clearTimeout(deadline);
    deadline = undefined;
  }

  function finishedFor(version: string | undefined) {
    return finished && finished.key === key && finished.version === version
      ? finished
      : undefined;
  }

  /** Whether the part has shown what a read of `version` led to. */
  function settledOn(version: string | undefined) {
    const shown = view.volume;
    return (
      (shown.state === "shown" || shown.state === "failed") &&
      shown.version === version
    );
  }

  /** Records a finished read and counts failures of the open part in a row. */
  function finish(done: Finished) {
    finished = done;
    if (done.key !== key) return;
    if (done.response) failures = 0;
    else if (!done.replaced) failures += 1;
  }

  function read(target: VolumeTarget, version: string) {
    const readKey = JSON.stringify(target);
    const requestSeq = ++sequence;
    reading = true;
    client
      .call("space.volume_gap", {
        resultVersion: version,
        requestSeq,
        target:
          "root" in target
            ? { root: { resultVersion: version, itemNo: target.root } }
            : { volumeId: target.volumeId },
      })
      .then(
        (response) => {
          const matches =
            response.resultVersion === version &&
            response.requestSeq === requestSeq;
          finish(
            matches
              ? { key: readKey, version, response, replaced: false }
              : { key: readKey, version, problem: MISMATCH, replaced: false },
          );
        },
        (failure: unknown) => {
          finish({
            key: readKey,
            version,
            problem: lookupProblem(failure),
            replaced:
              failure instanceof AnalysisError &&
              failure.code === "STALE_VERSION",
          });
        },
      )
      .finally(() => {
        reading = false;
        step();
      });
  }

  /** The frame the part keeps when its values give way to a line of text. */
  function frame(): { framed: boolean; volumeId: string | null } {
    const shown = view.volume;
    if (shown.state === "shown")
      return { framed: true, volumeId: shown.response.volumeId };
    if (shown.state === "updating")
      return { framed: true, volumeId: shown.volumeId };
    return shown.state === "failed"
      ? { framed: shown.framed, volumeId: shown.volumeId }
      : { framed: false, volumeId: null };
  }

  /** What the part shows when the summary changes while its read goes on. */
  function late(): VolumeView {
    return view.volume.state === "shown"
      ? { state: "updating", volumeId: frame().volumeId }
      : view.volume;
  }

  /**
   * What the part shows after a read of the summary's result failed. It
   * waits for the next result only while one is due and the part has values
   * to wait with; a part that shows a problem keeps showing one.
   */
  function failedView(done: Finished): VolumeView {
    const shown = view.volume;
    const waits = shown.state === "shown" || shown.state === "updating";
    const due =
      live &&
      (done.replaced || (waits && failures < VOLUME_READ_FAILURE_LIMIT));
    if (!due)
      return {
        state: "failed",
        version: done.version,
        problem: done.problem ?? MISMATCH,
        ...frame(),
      };
    if (shown.state === "failed") return { ...shown, version: done.version };
    return shown.state === "shown" ? late() : shown;
  }

  function outcome(done: Finished): VolumeView {
    return done.response
      ? { state: "shown", version: done.version, response: done.response }
      : failedView(done);
  }

  function settle(total: SpaceTotal, volume: VolumeView) {
    endBatch();
    commit({ total, target: view.target, volume });
    step();
  }

  function advance(target: VolumeTarget, next: SpaceTotal) {
    if (!batch) {
      if (cadence.remaining() > 0) {
        cadence.defer(step);
        return;
      }
      batch = { total: next, started: Date.now() };
    }
    const { total, started } = batch;
    const done = finishedFor(total.resultVersion);
    const left = started + volumePartPace(target) - Date.now();
    if (done) settle(total, outcome(done));
    else if (left <= 0) settle(total, late());
    else {
      // The limit follows the part that is open now, so it is armed anew.
      deadline ??= window.setTimeout(() => {
        deadline = undefined;
        step();
      }, left);
      if (!reading) read(target, total.resultVersion);
    }
  }

  /** Shows what a finished read of the result the summary shows led to. */
  function reflect(version: string | undefined) {
    const done = finishedFor(version);
    if (!done || settledOn(version)) return;
    const volume = outcome(done);
    if (volume !== view.volume) commit({ ...view, volume });
  }

  function step() {
    if (!alive || holding) return;
    const next = incoming;
    const target = view.target;
    if (!target || !next) {
      endBatch();
      if (view.total !== next || target)
        commit({ total: next, target: undefined, volume: CLOSED });
      return;
    }
    const current = view.total?.resultVersion;
    reflect(current);
    if (next.resultVersion !== current) {
      advance(target, next);
      return;
    }
    if (next !== view.total) commit({ ...view, total: next });
    // Brings the part to the result the summary already shows.
    if (!reading && !settledOn(current) && !finishedFor(current))
      read(target, next.resultVersion);
  }

  function reset() {
    failures = 0;
    finished = undefined;
  }

  return {
    offer(total, isLive) {
      incoming = total;
      live = isLive;
      step();
    },
    hold(on) {
      holding = on;
      step();
    },
    open(target) {
      key = JSON.stringify(target);
      reset();
      commit({ total: view.total, target, volume: READING });
      step();
    },
    close() {
      key = undefined;
      reset();
      endBatch();
      commit({ total: view.total, target: undefined, volume: CLOSED });
      step();
    },
    retry() {
      const shown = view.volume;
      if (shown.state !== "failed") return;
      reset();
      const volume: VolumeView = shown.framed
        ? { state: "updating", volumeId: shown.volumeId }
        : READING;
      commit({ ...view, volume });
      step();
    },
    dispose() {
      alive = false;
      endBatch();
      cadence.dispose();
    },
  };
}

/**
 * The summary's totals and volume part for the newest `total` of a scan.
 * `hold` is the text-selection hold; `live` is whether the scan will publish
 * a newer result (running and not paused). See `createSummaryBatch`.
 */
export function useSummaryBatch(
  client: AnalysisClient,
  total: SpaceTotal | undefined,
  hold: boolean,
  live: boolean,
): { total: SpaceTotal | undefined; volume: VolumePart } {
  const [view, setView] = useState<SummaryView>({
    total,
    target: undefined,
    volume: CLOSED,
  });
  const batch = useRef<SummaryBatch>(undefined);
  useLayoutEffect(() => {
    const created = createSummaryBatch(client, setView);
    batch.current = created;
    created.hold(hold);
    created.offer(total, live);
    return () => created.dispose();
    // Later values reach the batch through the effects below.
  }, [client]);
  useLayoutEffect(() => batch.current?.hold(hold), [hold]);
  useLayoutEffect(() => batch.current?.offer(total, live), [total, live]);
  return {
    total: view.total,
    volume: {
      target: view.target,
      view: view.volume,
      open: (target) => batch.current?.open(target),
      close: () => batch.current?.close(),
      retry: () => batch.current?.retry(),
    },
  };
}
