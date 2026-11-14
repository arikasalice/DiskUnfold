import { useCallback, useEffect, useRef, useState } from "react";
import { scanPriorityFailure } from "../ipc";
import type { AnalysisClient, ItemRef, ScanPriorityOutcome } from "../ipc";

/** A running scan this window started and its newest partial result version. */
export type ScanPriorityContext = { jobRef: string; resultVersion: string };

/** One outcome to announce; `id` changes for every announcement. */
export type ScanPriorityNotice = {
  id: number;
  outcome: ScanPriorityOutcome;
  name: string;
};

/** The "scan first" view state offered to a folder list. */
export type ScanPriority = {
  /** Whether this item belongs to the running scan's current partial result. */
  canRequest: (item: ItemRef) => boolean;
  /** Whether this item is the folder last requested for the running scan. */
  requested: (item: ItemRef) => boolean;
  request: (item: ItemRef, name: string) => void;
  notice: ScanPriorityNotice | undefined;
};

type Request = { seq: number; jobRef: string; item: ItemRef };

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
 */
export function useScanPriority(
  client: AnalysisClient | undefined,
  context: ScanPriorityContext | undefined,
): ScanPriority {
  const [marked, setMarked] = useState<Omit<Request, "seq">>();
  const [notice, setNotice] = useState<ScanPriorityNotice>();
  const sequence = useRef(0);
  const notices = useRef(0);
  const pending = useRef<Request>(undefined);
  const current = useRef(context);
  const jobRef = context?.jobRef;
  const resultVersion = context?.resultVersion;
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

  const unmark = useCallback((owner: Request) => {
    setMarked((shown) =>
      shown &&
      shown.jobRef === owner.jobRef &&
      sameItem(shown.item, owner.item) &&
      sequence.current === owner.seq
        ? undefined
        : shown,
    );
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
      setMarked({ jobRef: owner.jobRef, item });
      client
        .call("scan.prioritize", { jobRef: owner.jobRef, priorityArea: item })
        .then(
          (response) => {
            const outcome = outcomeOf(response.controlResult);
            const applied = response.appliedPriorityArea;
            const keep =
              outcome === "accepted" ||
              (outcome === "no_effect" &&
                applied !== undefined &&
                sameItem(applied, item));
            if (!keep) unmark(owner);
            settle(owner, outcome, name);
          },
          (failure: unknown) => {
            unmark(owner);
            settle(owner, scanPriorityFailure(failure), name);
          },
        );
    },
    [client, settle, unmark],
  );

  return {
    canRequest: (item) =>
      client !== undefined &&
      resultVersion !== undefined &&
      item.resultVersion === resultVersion,
    requested: (item) =>
      marked !== undefined &&
      marked.jobRef === jobRef &&
      sameItem(marked.item, item),
    request,
    notice,
  };
}
