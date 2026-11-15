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
};

type Pages = {
  rows: ScanFailureRow[];
  totalCount: number;
  nextCursor?: string | undefined;
};

/** One read: where it starts, how much it reads, and whether it replaces. */
type ReadPlan = {
  version: string;
  kind: ScanFailureKind | null;
  cursor: string | undefined;
  minimum: number;
  replace: boolean;
};

class PageMismatch extends Error {}

function emptyList(version: string, kind: ScanFailureKind | null): ListState {
  return { version, kind, rows: [], pending: true };
}

function owner(version: string, kind: ScanFailureKind | null): RequestOwner {
  return { version, kind, request: 0, inFlight: false };
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

/**
 * Reads pages from the plan's cursor until at least its minimum of rows or
 * the end. `wanted` is asked after every page: a read that became obsolete
 * requests nothing further and yields undefined.
 */
async function readPages(
  client: AnalysisClient,
  { version, kind, cursor, minimum }: ReadPlan,
  wanted: () => boolean,
): Promise<Pages | undefined> {
  const rows: ScanFailureRow[] = [];
  let next = cursor;
  for (;;) {
    const page = await client.call("scan.failures", {
      resultVersion: version,
      ...(kind && { kind }),
      ...(next && { cursor: next }),
    });
    if (!wanted()) return undefined;
    if (!pageMatches(page, version, kind)) throw new PageMismatch();
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
 * The list after `pages` arrived. A page is appended only to rows of the same
 * result that end at the cursor it was read from; cursors do not name their
 * result, so the result is compared as well.
 */
function withPages(
  previous: ListState,
  plan: ReadPlan,
  pages: Pages,
): ListState {
  const appended = plan.cursor !== undefined && !plan.replace;
  if (
    appended &&
    (previous.version !== plan.version || previous.nextCursor !== plan.cursor)
  )
    return { ...previous, pending: false };
  return {
    version: plan.version,
    kind: plan.kind,
    rows: appended ? [...previous.rows, ...pages.rows] : pages.rows,
    totalCount: pages.totalCount,
    nextCursor: pages.nextCursor,
    pending: false,
  };
}

/** The list after a read failed; a replacement refused as replaced is dropped. */
function withProblem(
  previous: ListState,
  plan: ReadPlan,
  problem: ResultReadProblem,
): ListState {
  if (!plan.replace)
    return { ...previous, pending: false, problem, failedCursor: plan.cursor };
  return problem === "stale_version"
    ? { ...previous, pending: false }
    : { ...previous, pending: false, problem, replaceFailed: true };
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
 *
 * "Load more" never appends to rows of an older result. While such rows are
 * shown it runs the replacement first, or joins the one in flight, and reads
 * the next page of the new result after it. A page still being read when the
 * result changes is owed the same way. The request survives a dropped
 * replacement and ends with the filter or a failed one.
 *
 * A response is applied only while its version, filter and request order
 * still match; an obsolete read stops asking for pages and its late
 * responses are dropped.
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
  /** "Load more" owed after the rows are replaced with the newer result. */
  const moreOwed = useRef(false);
  useEffect(() => {
    shown.current = list;
  });

  const read = useCallback(
    async (cursor: string | undefined, minimum: number, replace: boolean) => {
      const mine = current.current;
      if (mine.version !== resultVersion || mine.kind !== filter) return;
      if (mine.inFlight) return;
      const request = ++mine.request;
      mine.inFlight = true;
      const wanted = () => current.current === mine && mine.request === request;
      const plan: ReadPlan = {
        version: resultVersion,
        kind: filter,
        cursor,
        minimum,
        replace,
      };
      if (!replace)
        setList((previous) => ({
          ...previous,
          pending: true,
          problem: undefined,
          failedCursor: undefined,
          replaceFailed: undefined,
        }));
      let pages: Pages | undefined;
      let problem: ResultReadProblem | undefined;
      try {
        pages = await readPages(client, plan, wanted);
      } catch (failure) {
        problem = problemOf(failure);
      }
      if (!wanted()) return;
      mine.inFlight = false;
      if (!pages) {
        const failed = problem ?? "failed";
        if (failed !== "stale_version") moreOwed.current = false;
        setList((previous) => withProblem(previous, plan, failed));
        return;
      }
      const loaded = pages;
      setList((previous) => withPages(previous, plan, loaded));
      if (!replace || !moreOwed.current) return;
      moreOwed.current = false;
      if (loaded.nextCursor !== undefined)
        void read(loaded.nextCursor, 1, false);
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
    if (!replace) {
      moreOwed.current = false;
      setList(emptyList(resultVersion, filter));
    } else if (previous.pending) {
      // A page of the replaced result was being read; it is owed after this.
      moreOwed.current = true;
    }
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
      if (!view.nextCursor || problem) return;
      if (!replacing) {
        void read(view.nextCursor, 1, false);
        return;
      }
      // The cursor belongs to the older result; replace its rows first.
      moreOwed.current = true;
      setList((previous) => ({ ...previous, pending: true }));
      void read(undefined, view.rows.length, true);
    },
    retry: () =>
      view.version !== resultVersion
        ? void read(undefined, view.rows.length, true)
        : void read(view.failedCursor, 1, false),
  };
}
