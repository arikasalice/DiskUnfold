import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnalysisError, resultReadProblem } from "../ipc";
import type {
  AnalysisClient,
  ItemRef,
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
  readonly rootIndexesKey: string | null;
  readonly failuresOnly: boolean;
  /** The page to read again after a failure; undefined retries the first page. */
  readonly failedCursor?: string | undefined;
  /** Set when replacing the rows with a newer result failed. */
  readonly replaceFailed?: boolean | undefined;
};

type RequestOwner = {
  version: string;
  kind: ScanFailureKind | null;
  rootIndexesKey: string | null;
  failuresOnly: boolean;
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
  rootIndexes?: number[] | undefined;
  underItem?: ItemRef | undefined;
  failuresOnly: boolean;
  cursor: string | undefined;
  minimum: number;
  replace: boolean;
};

class PageMismatch extends Error {}

/** The storage and folder filters of a list as one key, so a change of either replaces it. */
function filterKey(roots: string | null, under: ItemRef | undefined) {
  return under === undefined ? roots : `${roots ?? ""}#${under.itemNo}`;
}

function emptyList(
  version: string,
  kind: ScanFailureKind | null,
  rootIndexesKey: string | null,
  failuresOnly: boolean,
): ListState {
  return {
    version,
    kind,
    rootIndexesKey,
    failuresOnly,
    rows: [],
    pending: true,
  };
}

function owner(
  version: string,
  kind: ScanFailureKind | null,
  rootIndexesKey: string | null,
  failuresOnly: boolean,
): RequestOwner {
  return {
    version,
    kind,
    rootIndexesKey,
    failuresOnly,
    request: 0,
    inFlight: false,
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

/**
 * Reads pages from the plan's cursor until at least its minimum of rows or
 * the end. `wanted` is asked after every page: a read that became obsolete
 * requests nothing further and yields undefined.
 */
async function readPages(
  client: AnalysisClient,
  {
    version,
    kind,
    rootIndexes,
    underItem,
    failuresOnly,
    cursor,
    minimum,
  }: ReadPlan,
  wanted: () => boolean,
): Promise<Pages | undefined> {
  const rows: ScanFailureRow[] = [];
  let next = cursor;
  for (;;) {
    const page = await client.call("scan.failures", {
      resultVersion: version,
      ...(kind && { kind }),
      ...(rootIndexes && { rootIndexes }),
      ...(underItem && { underItem }),
      ...(failuresOnly && { failuresOnly: true }),
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
    rootIndexesKey: filterKey(
      plan.rootIndexes?.join(",") ?? null,
      plan.underItem,
    ),
    failuresOnly: plan.failuresOnly,
    rows: appended ? [...previous.rows, ...pages.rows] : pages.rows,
    totalCount: pages.totalCount,
    nextCursor: pages.nextCursor,
    pending: false,
  };
}

/** Keeps displayed rows and exposes recovery when a page read is refused. */
function withProblem(
  previous: ListState,
  plan: ReadPlan,
  problem: ResultReadProblem,
): ListState {
  if (!plan.replace)
    return { ...previous, pending: false, problem, failedCursor: plan.cursor };
  return { ...previous, pending: false, problem, replaceFailed: true };
}

/**
 * Pages scan issues for the displayed result version and active filters.
 *
 * Changing a filter or refresh key discards the loaded rows and opens the first page.
 * When only the result version changes, the rows already shown stay until
 * the same number of rows has been read from the new version; then all of
 * them are replaced at once, so the list never empties and never mixes rows
 * from two results. A refused replacement keeps the old rows and offers
 * recovery; it is not retried until a new result version arrives.
 *
 * "Load more" never appends to rows of an older result. While such rows are
 * shown it runs the replacement first, or joins the one in flight, and reads
 * the next page of the new result after it. A page still being read when the
 * result changes is owed the same way. The request survives a refused
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
  rootIndexes?: readonly number[],
  failuresOnly = false,
  refreshKey = 0,
  underItem?: ItemRef,
) {
  const filter = kind ?? null;
  const rootsKey = rootIndexes?.join(",") ?? null;
  const rootIndexesKey = filterKey(rootsKey, underItem);
  const requestRoots = useMemo(
    () => rootsKey?.split(",").map(Number),
    [rootsKey],
  );
  const underNo = underItem?.itemNo;
  const underVersion = underItem?.resultVersion;
  const requestUnder = useMemo(
    () =>
      underNo === undefined || underVersion === undefined
        ? undefined
        : { resultVersion: underVersion, itemNo: underNo },
    [underNo, underVersion],
  );
  const [list, setList] = useState<ListState>(() =>
    emptyList(resultVersion, filter, rootIndexesKey, failuresOnly),
  );
  const shown = useRef(list);
  const current = useRef<RequestOwner>(
    owner(resultVersion, filter, rootIndexesKey, failuresOnly),
  );
  /** "Load more" owed after the rows are replaced with the newer result. */
  const moreOwed = useRef(false);
  useEffect(() => {
    shown.current = list;
  });

  const read = useCallback(
    async (cursor: string | undefined, minimum: number, replace: boolean) => {
      const mine = current.current;
      if (
        mine.version !== resultVersion ||
        mine.kind !== filter ||
        mine.rootIndexesKey !== rootIndexesKey ||
        mine.failuresOnly !== failuresOnly
      )
        return;
      if (mine.inFlight) return;
      const request = ++mine.request;
      mine.inFlight = true;
      const wanted = () => current.current === mine && mine.request === request;
      const plan: ReadPlan = {
        version: resultVersion,
        kind: filter,
        rootIndexes: requestRoots,
        underItem: requestUnder,
        failuresOnly,
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
      let failure: unknown;
      try {
        pages = await readPages(client, plan, wanted);
      } catch (caught) {
        failure = caught;
        problem = problemOf(caught);
      }
      if (!wanted()) return;
      mine.inFlight = false;
      if (cursor !== undefined && invalidCursor(failure)) {
        moreOwed.current = false;
        setList((previous) => ({
          ...previous,
          pending: true,
          problem: undefined,
          failedCursor: undefined,
          replaceFailed: undefined,
        }));
        void read(undefined, Math.max(1, shown.current.rows.length), true);
        return;
      }
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
    [
      client,
      failuresOnly,
      filter,
      requestRoots,
      requestUnder,
      resultVersion,
      rootIndexesKey,
    ],
  );

  useEffect(() => {
    const previous = shown.current;
    const replace =
      previous.kind === filter &&
      previous.rootIndexesKey === rootIndexesKey &&
      previous.failuresOnly === failuresOnly &&
      previous.version !== resultVersion &&
      previous.rows.length > 0;
    current.current = owner(
      resultVersion,
      filter,
      rootIndexesKey,
      failuresOnly,
    );
    if (!replace) {
      moreOwed.current = false;
      setList(emptyList(resultVersion, filter, rootIndexesKey, failuresOnly));
    } else if (previous.pending) {
      // A page of the replaced result was being read; it is owed after this.
      moreOwed.current = true;
    }
    void read(undefined, replace ? previous.rows.length : 1, replace);
    return () => {
      // Invalidate every request issued for this result and filter.
      current.current = owner("", null, null, false);
    };
  }, [read, filter, resultVersion, rootIndexesKey, failuresOnly, refreshKey]);

  const replacing = list.version !== resultVersion;
  const view =
    list.kind === filter &&
    list.rootIndexesKey === rootIndexesKey &&
    list.failuresOnly === failuresOnly &&
    (!replacing || list.rows.length > 0)
      ? list
      : emptyList(resultVersion, filter, rootIndexesKey, failuresOnly);
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
    version: view.version,
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

function invalidCursor(failure: unknown) {
  return (
    failure instanceof AnalysisError &&
    (failure.code === "STALE_VERSION" ||
      failure.reasonKey === "cursor.stale" ||
      failure.reasonKey === "cursor.invalid")
  );
}
