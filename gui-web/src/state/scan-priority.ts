import { useCallback, useEffect, useRef, useState } from "react";
import { scanPriorityFailure } from "../ipc";
import type { AnalysisClient, ItemRef, ScanPriorityOutcome } from "../ipc";

/**
 * A running scan this window started and its newest partial result version.
 * `scanning` is true only while the job state is `running`; a paused or
 * pausing scan is not working on any folder.
 */
export type ScanPriorityContext = {
  jobRef: string;
  resultVersion: string;
  scanning?: boolean;
};

/** One outcome to announce; `id` changes for every announcement. */
export type ScanPriorityNotice = {
  id: number;
  outcome: ScanPriorityOutcome;
  name: string;
};

/**
 * How a folder's "Analyze first" request is shown: sent and not yet confirmed
 * as the applied area, or confirmed while the scan is running.
 */
export type ScanPriorityStatus = "requested" | "analyzing";

/** The "scan first" view state offered to a folder list. */
export type ScanPriority = {
  /** Whether this item belongs to the running scan's current partial result. */
  canRequest: (item: ItemRef) => boolean;
  /** Whether this item is the folder last requested for the running scan. */
  requested: (item: ItemRef) => boolean;
  /** The request state shown for this item, if it is the requested folder. */
  status: (item: ItemRef) => ScanPriorityStatus | undefined;
  request: (item: ItemRef, name: string) => void;
  notice: ScanPriorityNotice | undefined;
};

type Request = { seq: number; jobRef: string; item: ItemRef };
type Marked = Omit<Request, "seq"> & { applied: boolean };

function sameItem(left: ItemRef, right: ItemRef): boolean {
  return (
    left.resultVersion === right.resultVersion && left.itemNo === right.itemNo
  );
}

function outcomeOf(result: string): ScanPriorityOutcome {
  return result === "accepted" ||
    result === "no_effect" ||
    result === "already_terminal"
    ? result
    : "failed";
}

/**
 * Sends scan.prioritize for a folder of the running scan's current partial
 * result (F02 FR8.HD1, HLD IF-03).
 *
 * The row is marked as requested before the response. "accepted" only confirms
 * receipt; the engine applies the area from its next assignment. A repeated
 * request for the same folder is not sent while one is in flight. A response
 * is dropped when a later request superseded it or the result it was pinned to
 * has been replaced in the same scan.
 *
 * The row reads "analyzing first" only after a response names the folder as
 * the applied priority area and while the scan is running.
 */
export function useScanPriority(
  client: AnalysisClient | undefined,
  context: ScanPriorityContext | undefined,
): ScanPriority {
  const [marked, setMarked] = useState<Marked>();
  const [notice, setNotice] = useState<ScanPriorityNotice>();
  const sequence = useRef(0);
  const notices = useRef(0);
  const pending = useRef<Request>(undefined);
  const current = useRef(context);
  const jobRef = context?.jobRef;
  const resultVersion = context?.resultVersion;
  const scanning = context?.scanning === true;
  useEffect(() => {
    current.current =
      jobRef && resultVersion ? { jobRef, resultVersion } : undefined;
  }, [jobRef, resultVersion]);

  const settle = useCallback(
    (owner: Request, outcome: ScanPriorityOutcome, name: string) => {
      if (sequence.current !== owner.seq) return;
      pending.current = undefined;
      const now = current.current;
      // A replaced result in the same or another scan makes the answer obsolete.
      if (
        now &&
        (now.jobRef !== owner.jobRef ||
          now.resultVersion !== owner.item.resultVersion)
      )
        return;
      setNotice({ id: ++notices.current, outcome, name });
    },
    [],
  );

  /** Updates the mark only if it still belongs to this request. */
  const remark = useCallback((owner: Request, applied: boolean | undefined) => {
    setMarked((shown) => {
      if (
        !shown ||
        shown.jobRef !== owner.jobRef ||
        !sameItem(shown.item, owner.item) ||
        sequence.current !== owner.seq
      )
        return shown;
      return applied === undefined ? undefined : { ...shown, applied };
    });
  }, []);

  const request = useCallback(
    (item: ItemRef, name: string) => {
      const scan = current.current;
      if (!client || !scan || item.resultVersion !== scan.resultVersion) return;
      const inFlight = pending.current;
      if (
        inFlight &&
        inFlight.jobRef === scan.jobRef &&
        sameItem(inFlight.item, item)
      )
        return;
      const owner = { seq: ++sequence.current, jobRef: scan.jobRef, item };
      pending.current = owner;
      setMarked({ jobRef: owner.jobRef, item, applied: false });
      client
        .call("scan.prioritize", { jobRef: owner.jobRef, priorityArea: item })
        .then(
          (response) => {
            const outcome = outcomeOf(response.controlResult);
            const applied = response.appliedPriorityArea;
            const isApplied = applied !== undefined && sameItem(applied, item);
            const keep =
              outcome === "accepted" || (outcome === "no_effect" && isApplied);
            remark(owner, keep ? isApplied : undefined);
            settle(owner, outcome, name);
          },
          (failure: unknown) => {
            remark(owner, undefined);
            settle(owner, scanPriorityFailure(failure), name);
          },
        );
    },
    [client, settle, remark],
  );

  const mine = (item: ItemRef) =>
    marked !== undefined &&
    marked.jobRef === jobRef &&
    sameItem(marked.item, item);
  return {
    canRequest: (item) =>
      client !== undefined &&
      resultVersion !== undefined &&
      item.resultVersion === resultVersion,
    requested: mine,
    status: (item) =>
      !mine(item)
        ? undefined
        : marked?.applied && scanning
          ? "analyzing"
          : "requested",
    request,
    notice,
  };
}
