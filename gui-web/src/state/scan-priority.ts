import { useCallback, useEffect, useRef, useState } from "react";
import { scanPriorityFailure } from "../ipc";
import type {
  AnalysisClient,
  ItemRef,
  ScanPriorityAnnouncement,
  ScanPriorityOutcome,
} from "../ipc";

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

/** How long an outcome stays available to announce, the notice display time. */
export const SCAN_PRIORITY_NOTICE_MS = 4_000;

/**
 * One outcome to announce; `id` changes for every announcement and `jobRef`
 * names the scan the request was sent to.
 */
export type ScanPriorityNotice = ScanPriorityAnnouncement & {
  id: number;
  jobRef: string;
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

/** `previous` names the folder shown as requested when this request replaced it. */
type Request = {
  seq: number;
  jobRef: string;
  item: ItemRef;
  name: string;
  previous: string | undefined;
};
type Marked = Request & { applied: boolean };

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
 *
 * The notice says what the response confirmed: whether the folder is the
 * applied area, whether the scan was paused, and which requested folder the
 * request replaced.
 *
 * An outcome is offered for announcement only for a short time and only while
 * no other scan is current, so a view that mounts later, or a later scan, does
 * not announce it again.
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
  const shown = useRef<Marked>(undefined);
  const current = useRef<ScanPriorityContext>(undefined);
  const jobRef = context?.jobRef;
  const resultVersion = context?.resultVersion;
  const scanning = context?.scanning === true;
  useEffect(() => {
    current.current =
      jobRef && resultVersion ? { jobRef, resultVersion, scanning } : undefined;
  }, [jobRef, resultVersion, scanning]);

  const mark = useCallback((next: Marked | undefined) => {
    shown.current = next;
    setMarked(next);
  }, []);

  const settle = useCallback(
    (owner: Request, outcome: ScanPriorityOutcome, applied: boolean) => {
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
      setNotice({
        id: ++notices.current,
        jobRef: owner.jobRef,
        outcome,
        name: owner.name,
        applied,
        paused: now !== undefined && now.scanning !== true,
        previous: owner.previous,
      });
    },
    [],
  );

  const noticeId = notice?.id;
  useEffect(() => {
    if (noticeId === undefined) return;
    const timer = window.setTimeout(
      () => setNotice((shown) => (shown?.id === noticeId ? undefined : shown)),
      SCAN_PRIORITY_NOTICE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [noticeId]);

  /** Updates the mark only if it still belongs to this request. */
  const remark = useCallback(
    (owner: Request, applied: boolean | undefined) => {
      const now = shown.current;
      if (now?.seq !== owner.seq) return;
      mark(applied === undefined ? undefined : { ...now, applied });
    },
    [mark],
  );

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
      const before = shown.current;
      const replaced =
        before !== undefined &&
        before.jobRef === scan.jobRef &&
        before.item.resultVersion === item.resultVersion &&
        !sameItem(before.item, item);
      const owner: Request = {
        seq: ++sequence.current,
        jobRef: scan.jobRef,
        item,
        name,
        previous: replaced ? before.name : undefined,
      };
      pending.current = owner;
      mark({ ...owner, applied: false });
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
            settle(owner, outcome, isApplied);
          },
          (failure: unknown) => {
            remark(owner, undefined);
            settle(owner, scanPriorityFailure(failure), false);
          },
        );
    },
    [client, settle, remark, mark],
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
    // An ended scan leaves no current job and still announces its outcome.
    notice:
      notice && (jobRef === undefined || notice.jobRef === jobRef)
        ? notice
        : undefined,
  };
}
