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

/**
 * How long an outcome stays available to announce, which is the time a view
 * shows it, and how long an unconfirmed receipt keeps its "requested" mark.
 */
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
  /**
   * Whether this folder's request is still unanswered or confirmed as the
   * applied area, so "Analyze first" is not offered for it again.
   */
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

/**
 * `pending` is unanswered, `received` was accepted without naming the folder
 * as the applied area and `applied` names it as that area.
 */
type Marked = Request & { state: "pending" | "received" | "applied" };

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

/** The mark a response leaves on its folder, or none. */
function answeredState(
  outcome: ScanPriorityOutcome,
  applied: boolean,
): Marked["state"] | undefined {
  if (applied && (outcome === "accepted" || outcome === "no_effect"))
    return "applied";
  return outcome === "accepted" ? "received" : undefined;
}

/**
 * Sends scan.prioritize for a folder of the running scan's current partial
 * result (F02 FR8.HD1, HLD IF-03).
 *
 * The row is marked as requested before the response. "accepted" only confirms
 * receipt; the engine applies the area from its next assignment. The row reads
 * "analyzing first" only after a response names the folder as the applied
 * priority area and while the scan is running.
 *
 * One rule decides when a folder can be requested again. While its request is
 * unanswered, or after a response named it as the applied area, it is not sent
 * again. A receipt that does not name the area keeps the "requested" mark
 * until {@link SCAN_PRIORITY_NOTICE_MS} after the answer, the time its notice
 * is shown, or until the scan's current result is replaced, whichever comes
 * first, and then returns to neutral; the folder can be requested again as
 * soon as the answer arrives. A mark is shown only on rows of the result it
 * was requested in.
 *
 * Requests for different folders are sent as they are made, and the engine
 * keeps the last one it receives as the scan's single priority area. The
 * native bridge posts each call when it is made; the Windows host dispatches
 * scan.prioritize on one FIFO job-control lane, and the Linux and macOS hosts
 * dispatch messages one at a time on the main thread. The engine therefore
 * receives requests in the order they were made. A response to a request
 * that a later one replaced is dropped without a notice, even a failure or a
 * stale result: the later request decides the area, and its own response
 * reports the outcome the user is waiting for. A response is also dropped
 * when the result it was pinned to has been replaced in the same scan.
 *
 * The notice says what the response confirmed: whether the folder is the
 * applied area, whether the scan was paused, and which requested folder the
 * request replaced. An outcome is offered for announcement for
 * {@link SCAN_PRIORITY_NOTICE_MS} and only while no other scan is current, so
 * a view that mounts later, or a later scan, does not announce it again.
 * An outcome that says the area applies when the paused scan continues is
 * withdrawn once the scan continues, and stays withdrawn if the scan then
 * ends while it would still be shown.
 */
export function useScanPriority(
  client: AnalysisClient | undefined,
  context: ScanPriorityContext | undefined,
): ScanPriority {
  const [marked, setMarked] = useState<Marked>();
  const [notice, setNotice] = useState<ScanPriorityNotice>();
  const sequence = useRef(0);
  const notices = useRef(0);
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

  // A paused outcome no longer describes a scan that has continued.
  useEffect(() => {
    if (scanning) setNotice((shown) => (shown?.paused ? undefined : shown));
  }, [scanning]);

  const received = marked?.state === "received" ? marked.seq : undefined;
  useEffect(() => {
    if (received === undefined) return;
    const timer = window.setTimeout(() => {
      if (shown.current?.seq === received) mark(undefined);
    }, SCAN_PRIORITY_NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [received, mark]);
  useEffect(() => {
    const now = shown.current;
    if (now?.state === "received" && now.item.resultVersion !== resultVersion)
      mark(undefined);
  }, [resultVersion, mark]);

  /** Updates the mark only if it still belongs to this request. */
  const remark = useCallback(
    (owner: Request, state: Marked["state"] | undefined) => {
      const now = shown.current;
      if (now?.seq !== owner.seq) return;
      mark(state === undefined ? undefined : { ...now, state });
    },
    [mark],
  );

  const request = useCallback(
    (item: ItemRef, name: string) => {
      const scan = current.current;
      if (!client || !scan || item.resultVersion !== scan.resultVersion) return;
      const before = shown.current;
      const sameScan =
        before !== undefined &&
        before.jobRef === scan.jobRef &&
        before.item.resultVersion === item.resultVersion;
      if (
        sameScan &&
        sameItem(before.item, item) &&
        before.state !== "received"
      )
        return;
      const owner: Request = {
        seq: ++sequence.current,
        jobRef: scan.jobRef,
        item,
        name,
        previous:
          sameScan && !sameItem(before.item, item) ? before.name : undefined,
      };
      mark({ ...owner, state: "pending" });
      client
        .call("scan.prioritize", { jobRef: owner.jobRef, priorityArea: item })
        .then(
          (response) => {
            const outcome = outcomeOf(response.controlResult);
            const area = response.appliedPriorityArea;
            const applied = area !== undefined && sameItem(area, item);
            remark(owner, answeredState(outcome, applied));
            settle(owner, outcome, applied);
          },
          (failure: unknown) => {
            remark(owner, undefined);
            settle(owner, scanPriorityFailure(failure), false);
          },
        );
    },
    [client, settle, remark, mark],
  );

  const mine = (item: ItemRef): boolean =>
    marked !== undefined &&
    marked.jobRef === jobRef &&
    sameItem(marked.item, item);
  return {
    canRequest: (item) =>
      client !== undefined &&
      resultVersion !== undefined &&
      item.resultVersion === resultVersion,
    requested: (item) => mine(item) && marked?.state !== "received",
    status: (item) => {
      if (!mine(item)) return undefined;
      return marked?.state === "applied" && scanning
        ? "analyzing"
        : "requested";
    },
    request,
    // An ended scan leaves no current job and still announces its outcome.
    notice:
      notice && (jobRef === undefined || notice.jobRef === jobRef)
        ? notice
        : undefined,
  };
}
