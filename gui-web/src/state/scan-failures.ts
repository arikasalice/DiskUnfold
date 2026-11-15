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
  /** The result version and filter every shown row was read from. */
  readonly version: string;
  readonly kind: ScanFailureKind | null;
  /** The page to read again after a failure; undefined retries the first page. */
  readonly failedCursor?: string | undefined;
  /** Set when replacing the rows with a newer result failed. */
  readonly replaceFailed?: boolean | undefined;
};

type RequestOwner = {
  version: string;
  kind: ScanFailureKind | null;
  request: number;
  inFlight: boolean;
  /** "Load more" pressed while the rows were being replaced. */
  moreAfterReplace: boolean;
};

type Pages = {
  rows: ScanFailureRow[];
  totalCount: number;
  nextCursor?: string | undefined;
};

class PageMismatch extends Error {}

function emptyList(version: string, kind: ScanFailureKind | null): ListState {
  return { version, kind, rows: [], pending: true };
}

function owner(version: string, kind: ScanFailureKind | null): RequestOwner {
  return {
    version,
    kind,
    request: 0,
    inFlight: false,
    moreAfterReplace: false,
  };
}

function pageMatches(
  page: ScanFailures,
  resultVersion: string,
  kind: ScanFailureKind | null,
) {
  return (
    page.resultVersion === resultVersion &&
    (!kind || page.rows.every((row) => row.kind === kind))
  );
}

/** Reads pages from `cursor` until at least `minimum` rows or the end. */
async function readPages(
  client: AnalysisClient,
  resultVersion: string,
  kind: ScanFailureKind | null,
  cursor: string | undefined,
  minimum: number,
): Promise<Pages> {
  const rows: ScanFailureRow[] = [];
  let next = cursor;
  for (;;) {
    const page = await client.call("scan.failures", {
      resultVersion,
      ...(kind && { kind }),
      ...(next && { cursor: next }),
    });
    if (!pageMatches(page, resultVersion, kind)) throw new PageMismatch();
    rows.push(...page.rows);
    next = page.nextCursor;
    if (!next || rows.length >= minimum)
      return { rows, totalCount: page.totalCount, nextCursor: next };
  }
}

function problemOf(failure: unknown): ResultReadProblem {
  return failure instanceof PageMismatch
    ? "failed"
    : resultReadProblem(failure);
}

/**
 * Pages scan issues for the displayed result version and a kind filter.
 *
 * Changing the filter discards the loaded rows and opens the first page.
 * When only the result version changes, the rows already shown stay until
 * the same number of rows has been read from the new version; then all of
 * them are replaced at once, so the list never empties and never mixes rows
 * from two results. A replacement refused because the version was replaced
 * again is dropped quietly; the next version starts another one.
 * A response is applied only while its version, filter and request order
 * still match; late responses are dropped.
 */
export function useScanFailures(
  client: AnalysisClient,
  resultVersion: string,
  kind?: ScanFailureKind,
) {
  const filter = kind ?? null;
  const [list, setList] = useState<ListState>(() =>
    emptyList(resultVersion, filter),
  );
  const shown = useRef(list);
  const current = useRef<RequestOwner>(owner(resultVersion, filter));
  useEffect(() => {
    shown.current = list;
  });

  const read = useCallback(
    async (cursor: string | undefined, minimum: number, replace: boolean) => {
      const mine = current.current;
      if (mine.version !== resultVersion || mine.kind !== filter) return;
      if (mine.inFlight) {
        if (cursor !== undefined && !replace) mine.moreAfterReplace = true;
        return;
      }
      const request = ++mine.request;
      mine.inFlight = true;
      if (!replace)
        setList((previous) => ({
          ...previous,
          pending: true,
          problem: undefined,
          failedCursor: undefined,
          replaceFailed: undefined,
        }));
      let settle: (previous: ListState) => ListState;
      let after: string | undefined;
      try {
        const pages = await readPages(
          client,
          resultVersion,
          filter,
          cursor,
          minimum,
        );
        const appended = cursor !== undefined && !replace;
        settle = (previous) =>
          appended && previous.nextCursor !== cursor
            ? { ...previous, pending: false }
            : {
                version: resultVersion,
                kind: filter,
                rows: appended ? [...previous.rows, ...pages.rows] : pages.rows,
                totalCount: pages.totalCount,
                nextCursor: pages.nextCursor,
                pending: false,
              };
        if (replace && mine.moreAfterReplace) after = pages.nextCursor;
      } catch (failure) {
        const problem = problemOf(failure);
        settle = (previous) =>
          replace
            ? problem === "stale_version"
              ? previous
              : { ...previous, problem, replaceFailed: true }
            : { ...previous, pending: false, problem, failedCursor: cursor };
      }
      if (current.current !== mine || mine.request !== request) return;
      mine.inFlight = false;
      mine.moreAfterReplace = false;
      setList(settle);
      if (after !== undefined) void read(after, 1, false);
    },
    [client, filter, resultVersion],
  );

  useEffect(() => {
    const previous = shown.current;
    const replace =
      previous.kind === filter &&
      previous.version !== resultVersion &&
      previous.rows.length > 0;
    current.current = owner(resultVersion, filter);
    if (!replace) setList(emptyList(resultVersion, filter));
    void read(undefined, replace ? previous.rows.length : 1, replace);
    return () => {
      // Invalidate every request issued for this result and filter.
      current.current = owner("", null);
    };
  }, [read, filter, resultVersion]);

  const replacing = list.version !== resultVersion;
  const view =
    list.kind === filter && (!replacing || list.rows.length > 0)
      ? list
      : emptyList(resultVersion, filter);
  const problem =
    view.version === resultVersion || view.replaceFailed
      ? view.problem
      : undefined;
  return {
    rows: view.rows,
    totalCount: view.totalCount,
    nextCursor: view.nextCursor,
    pending: view.pending,
    problem,
    readMore: () => {
      if (view.nextCursor && !problem) void read(view.nextCursor, 1, false);
    },
    retry: () =>
      view.version !== resultVersion
        ? void read(undefined, view.rows.length, true)
        : void read(view.failedCursor, 1, false),
  };
}
