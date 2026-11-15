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
 * How long the summary waits for an open part to read the result it is about
 * to show. After that, or when the read fails, the summary changes alone and
 * the part says it is updating, so a slow read never makes the scan look
 * stopped (F02 design review of PR #69).
 */
export const SUMMARY_PART_WAIT_MS = 1000;

/** The scanned folder to explain, or the volume whose scanned folders to compare. */
export type VolumeTarget = { root: number } | { volumeId: string };

export function isWholeVolume(target: VolumeTarget | undefined): boolean {
  return target !== undefined && "volumeId" in target;
}

/** What the volume part of the summary shows. */
export type VolumeView =
  /** Not open. */
  | { state: "closed" }
  /** Opened, and nothing read for it yet. */
  | { state: "reading" }
  /** The summary moved to a result this part has not read yet. */
  | { state: "updating"; volumeId: string | null }
  /** What was read for `version`, the result the summary shows. */
  | {
      state: "shown";
      version: string;
      response?: VolumeGapResponse;
      problem?: Message;
    };

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
};

const CLOSED: VolumeView = { state: "closed" };
const READING: VolumeView = { state: "reading" };
const MISMATCH: Message = [
  "ui.analysis.volume_response_mismatch",
  "The response does not match this scan result.",
];

function paceOf(target: VolumeTarget): number {
  return isWholeVolume(target)
    ? VOLUME_COMPARE_REREAD_MS
    : RESULT_REFLECT_INTERVAL_MS;
}

/** A finished volume read; `replaced` when a newer result refused it. */
type Finished = {
  key: string;
  version: string;
  response?: VolumeGapResponse;
  problem?: Message;
  replaced: boolean;
};

function shownOf(done: Finished): VolumeView {
  return {
    state: "shown",
    version: done.version,
    ...(done.response ? { response: done.response } : {}),
    ...(done.problem ? { problem: done.problem } : {}),
  };
}

export type SummaryBatch = {
  /** The newest totals read for the scan, and whether it may publish more. */
  offer(total: SpaceTotal | undefined, live: boolean): void;
  /** Holds every value while the user selects text in the summary. */
  hold(on: boolean): void;
  open(target: VolumeTarget): void;
  close(): void;
  dispose(): void;
};

/**
 * Changes the summary's totals and its open volume part together, so the
 * summary never shows values from two results.
 *
 * With the part closed, offered totals are shown at once. With it open, a
 * newer total starts a batch: the part reads the batch's result and both
 * change in one view when the read ends. A batch starts at most once per the
 * pace of the open part and waits at most `SUMMARY_PART_WAIT_MS`; after that,
 * or when the read fails, the summary changes alone and the part is
 * "updating" until a read of the shown result ends. Reads never overlap. A
 * newer total offered during a batch does not restart it; the next batch
 * takes the newest total. A part whose read failed waits for the next result
 * while the scan is live; otherwise it shows the problem.
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
  let sequence = 0;
  const cadence = createReflectCadence(() =>
    slowestReflectInterval(view.target ? [paceOf(view.target)] : []),
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
          finished = matches
            ? { key: readKey, version, response, replaced: false }
            : { key: readKey, version, problem: MISMATCH, replaced: false };
        },
        (failure: unknown) => {
          finished = {
            key: readKey,
            version,
            problem: lookupProblem(failure),
            replaced:
              failure instanceof AnalysisError &&
              failure.code === "STALE_VERSION",
          };
        },
      )
      .finally(() => {
        reading = false;
        step();
      });
  }

  function shownVolumeId(): string | null {
    const shown = view.volume;
    if (shown.state === "updating") return shown.volumeId;
    return shown.state === "shown" ? (shown.response?.volumeId ?? null) : null;
  }

  /** What the part shows when the summary changes without it. */
  function late(done: Finished | undefined): VolumeView {
    if (view.volume.state !== "reading")
      return { state: "updating", volumeId: shownVolumeId() };
    // Nothing was shown yet, so there is no older value to clear.
    return done && !(live && done.replaced) ? shownOf(done) : READING;
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
      deadline = window.setTimeout(step, SUMMARY_PART_WAIT_MS);
    }
    const { total, started } = batch;
    const done = finishedFor(total.resultVersion);
    if (done?.response) settle(total, shownOf(done));
    else if (done || Date.now() - started >= SUMMARY_PART_WAIT_MS)
      settle(total, late(done));
    else if (!reading) read(target, total.resultVersion);
  }

  /** Brings the part to the result the summary already shows. */
  function catchUp(target: VolumeTarget, version: string) {
    const shown = view.volume;
    if (shown.state === "shown" && shown.version === version) return;
    if (reading) return;
    const done = finishedFor(version);
    if (!done) read(target, version);
    else if (
      done.response ||
      !live ||
      (!done.replaced && shown.state === "reading")
    )
      commit({ ...view, volume: shownOf(done) });
    // Otherwise the next result is on its way and its batch reads again.
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
    // A read that ended after the summary moved on still fits what it shows.
    const current = view.total?.resultVersion;
    const done = finishedFor(current);
    const shown = view.volume;
    if (
      done?.response &&
      !(shown.state === "shown" && shown.version === current)
    )
      commit({ ...view, volume: shownOf(done) });
    if (next.resultVersion !== current) {
      advance(target, next);
      return;
    }
    if (next !== view.total) commit({ ...view, total: next });
    catchUp(target, next.resultVersion);
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
      finished = undefined;
      commit({ total: view.total, target, volume: READING });
      step();
    },
    close() {
      key = undefined;
      endBatch();
      commit({ total: view.total, target: undefined, volume: CLOSED });
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
 * `hold` is the text-selection hold; `live` is whether the scan may still
 * publish a newer result. See `createSummaryBatch`.
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
    },
  };
}
