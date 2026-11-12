import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import type { BrowseGridFocus } from "../../components";
import { AnalysisError } from "../../ipc";
import type { QueryPageRequest, QueryPageResponse } from "../../ipc";
import {
  BROWSE_PAGE_SIZE,
  browsePageStart,
  visibleBrowsePages,
} from "../../state/browse-pages";
import type { BrowsePages } from "../../state/browse-pages";
import { useLocaleText } from "../../i18n/presentation";

const abandoned = Symbol("abandoned query page");
const scrollKeys = new Set([
  "ArrowUp",
  "ArrowDown",
  "PageUp",
  "PageDown",
  "Home",
  "End",
  " ",
]);

type Continuation = { start: number; cursor: string };
type PageRead = { nextCursor: string | null; length: number };
type Task = {
  start: number;
  continuation?: Continuation;
  target?: number;
};
type Intent = { origin: Element; scrollY: number };
type Failure = { task: Task; stale: boolean };
type FocusRequest = BrowseGridFocus & { keepLoadButton: boolean };
type BrowseStatusKey =
  | "additional_items_ready"
  | "no_confirmed_items_end_unknown"
  | "list_end_unknown";

const statusFallback: Record<BrowseStatusKey, string> = {
  additional_items_ready: "Additional items are ready.",
  no_confirmed_items_end_unknown:
    "No confirmed items. The end of the list is unknown.",
  list_end_unknown: "The end of the list is unknown.",
};

type Options = {
  scrollRoot: RefObject<HTMLDivElement | null>;
  pages: BrowsePages;
  initialCursor: string | null;
  readPage: (request: QueryPageRequest) => Promise<QueryPageResponse>;
  enabled: boolean;
  navigationEpoch: number;
  navigationSeq: () => number;
  canRead: () => boolean;
  focusPanel: () => void;
};

export function useBrowsePaging({
  scrollRoot,
  pages,
  initialCursor,
  readPage,
  enabled,
  navigationEpoch,
  navigationSeq,
  canRead,
  focusPanel,
}: Options) {
  const text = useLocaleText();
  const [continuation, setContinuation] = useState<Continuation | null>(() =>
    initialCursor
      ? {
          start: Math.min(pages.count, BROWSE_PAGE_SIZE),
          cursor: initialCursor,
        }
      : null,
  );
  const [focusRequest, setFocusRequest] = useState<FocusRequest>();
  const [explicit, setExplicit] = useState<Task>();
  const [failure, setFailure] = useState<Failure>();
  const [retrying, setRetrying] = useState(false);
  const [statusKey, setStatusKey] = useState<BrowseStatusKey>();
  const [background, setBackground] = useState(false);
  const [, refresh] = useState(0);
  const loadButton = useRef<HTMLButtonElement>(null);
  const retryButton = useRef<HTMLButtonElement>(null);
  const live = useRef(true);
  const intent = useRef<Intent>(undefined);
  const operation = useRef(0);
  const focusGeneration = useRef(0);
  const explicitRef = useRef<Task>(undefined);
  const stale = useRef(false);
  const focused = useRef<number>(undefined);
  const targetPin = useRef<number>(undefined);
  const lastEpoch = useRef(navigationEpoch);
  const visible = useRef<number[]>([]);
  const attempted = useRef(new Set<number>());
  const backgroundRead = useRef(false);
  const reads = useRef(
    new Map<number, { epoch: number; promise: Promise<PageRead> }>(),
  );

  function isCurrent(epoch: number) {
    return live.current && navigationSeq() === epoch && canRead();
  }

  function eligible(candidate: Intent | undefined) {
    return (
      candidate !== undefined &&
      intent.current === candidate &&
      candidate.origin.isConnected &&
      document.activeElement === candidate.origin &&
      (scrollRoot.current?.scrollTop ?? 0) === candidate.scrollY
    );
  }

  useLayoutEffect(() => {
    live.current = true;
    const root = scrollRoot.current;
    function cancelIntent() {
      intent.current = undefined;
    }
    function onFocus(event: FocusEvent) {
      focusGeneration.current += 1;
      if (event.target !== intent.current?.origin) cancelIntent();
    }
    function onBlur(event: FocusEvent) {
      focusGeneration.current += 1;
      if (event.target === intent.current?.origin) cancelIntent();
    }
    function onScroll() {
      if ((scrollRoot.current?.scrollTop ?? 0) !== intent.current?.scrollY)
        cancelIntent();
    }
    function onKey(event: globalThis.KeyboardEvent) {
      if (
        !event.isComposing &&
        event.keyCode !== 229 &&
        scrollKeys.has(event.key)
      )
        cancelIntent();
    }
    const input = ["wheel", "touchstart", "touchmove", "pointerdown"] as const;
    for (const event of input)
      window.addEventListener(event, cancelIntent, true);
    window.addEventListener("focusin", onFocus, true);
    window.addEventListener("focusout", onBlur, true);
    root?.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("keydown", onKey, true);
    return () => {
      live.current = false;
      operation.current += 1;
      cancelIntent();
      for (const event of input)
        window.removeEventListener(event, cancelIntent, true);
      window.removeEventListener("focusin", onFocus, true);
      window.removeEventListener("focusout", onBlur, true);
      root?.removeEventListener("scroll", onScroll);
      window.removeEventListener("keydown", onKey, true);
    };
  }, []);

  function recordLayoutScroll(before: number, after: number) {
    const current = intent.current;
    if (current?.scrollY === before) current.scrollY = after;
  }

  function read(task: Task, epoch: number): Promise<PageRead> {
    const { start, continuation } = task;
    const inFlight = reads.current.get(start);
    if (inFlight?.epoch === epoch) return inFlight.promise;
    const nextCursor = pages.nextCursor(start);
    if (nextCursor !== undefined && start < pages.count) {
      pages.touch(start);
      return Promise.resolve({
        nextCursor,
        length: Math.min(BROWSE_PAGE_SIZE, pages.count - start),
      });
    }
    const request = continuation
      ? { nextCursor: continuation.cursor, count: BROWSE_PAGE_SIZE }
      : pages.request(start);
    const promise = Promise.resolve()
      .then(async () => {
        if (!isCurrent(epoch) || stale.current) throw abandoned;
        const page = await readPage(request);
        if (!isCurrent(epoch) || stale.current) throw abandoned;
        if (!pages.matches(page))
          throw new Error("The folder page does not match its query.");
        if (
          continuation &&
          start === pages.count &&
          page.rows.length === 0 &&
          page.nextCursor === null
        )
          return { nextCursor: null, length: 0 };
        const protectedStarts = new Set([...visible.current, start]);
        if (focused.current !== undefined)
          protectedStarts.add(browsePageStart(focused.current));
        if (targetPin.current !== undefined)
          protectedStarts.add(targetPin.current);
        if (explicitRef.current) protectedStarts.add(explicitRef.current.start);
        if (!pages.accept(start, page, protectedStarts))
          throw new Error("The folder page does not match its position.");
        refresh((revision) => revision + 1);
        return { nextCursor: page.nextCursor, length: page.rows.length };
      })
      .finally(() => {
        if (reads.current.get(start)?.promise === promise)
          reads.current.delete(start);
      });
    reads.current.set(start, { epoch, promise });
    return promise;
  }

  function fail(task: Task, error: unknown) {
    if (error === abandoned) return;
    const expired =
      error instanceof AnalysisError && error.code === "STALE_VERSION";
    if (expired) stale.current = true;
    setFailure({ task, stale: expired });
  }

  function pumpVisible() {
    if (backgroundRead.current || stale.current || !canRead()) return;
    const start = visible.current.find(
      (page) => !pages.has(page) && !attempted.current.has(page),
    );
    if (start === undefined) return;
    attempted.current.add(start);
    backgroundRead.current = true;
    const epoch = navigationSeq();
    const requestOperation = operation.current;
    const task = { start };
    setBackground(true);
    void read(task, epoch)
      .catch((error: unknown) => {
        if (
          isCurrent(epoch) &&
          requestOperation === operation.current &&
          !explicitRef.current &&
          visible.current.includes(start)
        )
          fail(task, error);
      })
      .finally(() => {
        backgroundRead.current = false;
        if (!live.current) return;
        setBackground(false);
        if (!isCurrent(epoch)) attempted.current.delete(start);
        pumpVisible();
      });
  }

  function show(start: number, end: number) {
    const next = visibleBrowsePages(start, end, pages.count);
    if (
      next.length === visible.current.length &&
      next.every((page, index) => visible.current[index] === page)
    )
      return;
    visible.current = next;
    attempted.current = new Set(
      next.filter((page) => attempted.current.has(page)),
    );
    next.forEach((page) => pages.touch(page));
    pumpVisible();
  }

  useEffect(() => {
    if (!enabled || lastEpoch.current !== navigationEpoch) {
      lastEpoch.current = navigationEpoch;
      operation.current += 1;
      intent.current = undefined;
      explicitRef.current = undefined;
      targetPin.current = undefined;
      setExplicit(undefined);
      setRetrying(false);
      setFocusRequest(undefined);
      attempted.current.clear();
    }
    if (enabled) pumpVisible();
  }, [enabled, navigationEpoch]);

  function run(task: Task, keyboard: boolean, retryAttempt = false) {
    if (stale.current || !canRead()) return;
    const epoch = navigationSeq();
    const generation = ++operation.current;
    const active = document.activeElement;
    const originFocus = focusGeneration.current;
    const candidate =
      keyboard && active && active !== document.body
        ? { origin: active, scrollY: scrollRoot.current?.scrollTop ?? 0 }
        : undefined;
    intent.current = candidate;
    targetPin.current = undefined;
    explicitRef.current = task;
    setExplicit(task);
    setRetrying(retryAttempt);
    if (!retryAttempt) setFailure(undefined);
    setFocusRequest(undefined);
    function latest() {
      return isCurrent(epoch) && operation.current === generation;
    }
    void read(task, epoch)
      .then((page) => {
        if (!latest()) return;
        function finishControls() {
          const removed = retryAttempt
            ? retryButton.current
            : task.continuation && page.nextCursor === null
              ? loadButton.current
              : null;
          if (
            removed &&
            active === removed &&
            document.activeElement === removed &&
            originFocus === focusGeneration.current
          )
            focusPanel();
          if (retryAttempt) setFailure(undefined);
        }
        if (task.continuation) {
          if (page.length > 0 && !keyboard)
            setStatusKey("additional_items_ready");
          setContinuation(
            page.nextCursor
              ? { start: task.start + page.length, cursor: page.nextCursor }
              : null,
          );
        }
        if (
          task.target === undefined ||
          page.length === 0 ||
          !eligible(candidate)
        ) {
          finishControls();
          return;
        }
        targetPin.current = task.start;
        const request: FocusRequest = {
          index: task.target,
          keepLoadButton: task.continuation !== undefined,
          eligible: () => latest() && eligible(candidate),
          finish: () => {
            if (latest()) {
              finishControls();
              targetPin.current = undefined;
              if (intent.current === candidate) intent.current = undefined;
            }
            setFocusRequest((current) =>
              current === request ? undefined : current,
            );
          },
        };
        setFocusRequest(request);
      })
      .catch((error: unknown) => {
        if (latest()) fail(task, error);
      })
      .finally(() => {
        if (!latest()) return;
        explicitRef.current = undefined;
        setExplicit(undefined);
        setRetrying(false);
      });
  }

  function move(index: number, end: boolean) {
    if (end)
      setStatusKey(
        pages.rowCount === -1
          ? pages.count === 0
            ? "no_confirmed_items_end_unknown"
            : "list_end_unknown"
          : undefined,
      );
    if (index >= 0 && index < pages.count)
      run({ start: browsePageStart(index), target: index }, true);
  }

  function loadMore(keyboard: boolean) {
    if (!continuation || explicitRef.current?.continuation === continuation)
      return;
    run(
      { start: continuation.start, continuation, target: continuation.start },
      keyboard,
    );
  }

  function retry(keyboard: boolean) {
    if (!failure || failure.stale || explicitRef.current) return;
    run(failure.task, keyboard, true);
  }

  function trackFocus(index: number | undefined) {
    focused.current = index;
    if (index !== undefined) pages.touch(browsePageStart(index));
  }

  return {
    focusRequest,
    failure,
    status: statusKey
      ? text(`ui.browse.${statusKey}`, statusFallback[statusKey])
      : undefined,
    loadButton,
    retryButton,
    retrying,
    move,
    loadMore,
    retry,
    trackFocus,
    show,
    recordLayoutScroll,
    showLoadMore:
      continuation !== null || focusRequest?.keepLoadButton === true,
    loadingMore: explicit?.continuation !== undefined,
    moving: explicit !== undefined,
    pending: explicit !== undefined || background,
  };
}
