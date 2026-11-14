import { useCallback, useEffect, useRef, useState } from "react";
import { resultReadProblem } from "../ipc";
import type {
  AnalysisClient,
  ResultReadProblem,
  ScanFailureKind,
  ScanFailureRow,
  ScanFailures,
} from "../ipc";

/** Issue rows accumulated for one displayed result version and kind filter. */
export type ScanFailureList = {
  readonly rows: readonly ScanFailureRow[];
  /** Total rows for the result and filter, once the first page arrives. */
  readonly totalCount?: number | undefined;
  /** Continuation for the next page; absent when every row is loaded. */
  readonly nextCursor?: string | undefined;
  readonly pending: boolean;
  readonly problem?: ResultReadProblem | undefined;
};

type ListState = ScanFailureList & {
  readonly scope: string;
  /** The page to read again after a failure; undefined retries the first page. */
  readonly failedCursor?: string | undefined;
};

type RequestOwner = { scope: string; request: number; inFlight: boolean };

function listScope(resultVersion: string, kind: ScanFailureKind | undefined) {
  return JSON.stringify([resultVersion, kind ?? null]);
}

function emptyList(scope: string): ListState {
  return { scope, rows: [], pending: true };
}

function pageMatches(
  page: ScanFailures,
  resultVersion: string,
  kind: ScanFailureKind | undefined,
) {
  return (
    page.resultVersion === resultVersion &&
    (!kind || page.rows.every((row) => row.kind === kind))
  );
}

function appendPage(
  previous: ListState,
  page: ScanFailures,
  cursor: string | undefined,
): ListState {
  // A continuation belongs only directly after the page that issued its cursor.
  if (cursor !== undefined && previous.nextCursor !== cursor)
    return { ...previous, pending: false };
  return {
    scope: previous.scope,
    rows: cursor === undefined ? page.rows : [...previous.rows, ...page.rows],
    totalCount: page.totalCount,
    nextCursor: page.nextCursor,
    pending: false,
  };
}

function failPage(
  previous: ListState,
  problem: ResultReadProblem,
  cursor: string | undefined,
): ListState {
  return { ...previous, pending: false, problem, failedCursor: cursor };
}

/**
 * Pages scan issues for one pinned result version and kind filter.
 *
 * Changing either input discards the loaded rows and continuation and opens
 * the first page. A response is applied only while its result version, filter
 * and request order still match; late responses are dropped.
 */
export function useScanFailures(
  client: AnalysisClient,
  resultVersion: string,
  kind?: ScanFailureKind,
) {
  const scope = listScope(resultVersion, kind);
  const [list, setList] = useState<ListState>(() => emptyList(scope));
  const owner = useRef<RequestOwner>({ scope, request: 0, inFlight: false });

  const read = useCallback(
    async (cursor?: string) => {
      const current = owner.current;
      if (current.scope !== scope || current.inFlight) return;
      const request = ++current.request;
      current.inFlight = true;
      const update = (change: (previous: ListState) => ListState) =>
        setList((previous) =>
          previous.scope === scope ? change(previous) : previous,
        );
      update((previous) => ({
        ...previous,
        pending: true,
        problem: undefined,
        failedCursor: undefined,
      }));
      let settle: (previous: ListState) => ListState;
      try {
        const page = await client.call("scan.failures", {
          resultVersion,
          ...(kind && { kind }),
          ...(cursor && { cursor }),
        });
        settle = pageMatches(page, resultVersion, kind)
          ? (previous) => appendPage(previous, page, cursor)
          : (previous) => failPage(previous, "failed", cursor);
      } catch (failure) {
        const problem = resultReadProblem(failure);
        settle = (previous) => failPage(previous, problem, cursor);
      }
      if (owner.current !== current || current.request !== request) return;
      current.inFlight = false;
      update(settle);
    },
    [client, kind, resultVersion, scope],
  );

  useEffect(() => {
    owner.current = { scope, request: 0, inFlight: false };
    setList(emptyList(scope));
    void read();
    return () => {
      // Invalidate every request issued for this result and filter.
      owner.current = { scope: "", request: 0, inFlight: false };
    };
  }, [read, scope]);

  const view = list.scope === scope ? list : emptyList(scope);
  return {
    rows: view.rows,
    totalCount: view.totalCount,
    nextCursor: view.nextCursor,
    pending: view.pending,
    problem: view.problem,
    readMore: () => {
      if (view.nextCursor && !view.problem) void read(view.nextCursor);
    },
    retry: () => void read(view.failedCursor),
  };
}
