import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { KeyboardEvent } from "react";
import {
  Action,
  BrowseTable,
  canHandleBrowseKey,
  Heading,
  Notice,
  Progress,
  Text,
} from "../../components";
import type { BrowsePageFocus, PageViewport } from "../../components";
import type {
  BrowseOpenRequest,
  BrowseOpenResponse,
  BrowseValue,
  FolderQueryPage,
  FolderQueryRow,
  ItemRef,
  QueryPageRequest,
  QueryPageResponse,
  SizeValue,
} from "../../ipc";

/** Injected boundary for opening folders and paging through a pinned result. */
export interface BrowsePort {
  open(
    request: BrowseOpenRequest,
    signal?: AbortSignal,
  ): Promise<BrowseOpenResponse>;
  page(request: QueryPageRequest): Promise<QueryPageResponse>;
}

type Opened = Extract<BrowseOpenResponse, { kind: "opened" }>;
type View = {
  opened: Opened;
  rows: FolderQueryRow[];
  nextCursor: string | null;
  pageFocus?: BrowsePageFocus | undefined;
};

function name(value: BrowseValue<string>): string {
  return value.state === "present"
    ? value.value
    : value.state === "absent"
      ? "Not recorded"
      : "Unavailable";
}
function size(value: BrowseValue<SizeValue>): string {
  if (value.state === "absent") return "Not recorded";
  if (value.state !== "present" || value.value.state === "unknown")
    return "Unavailable";
  return `${value.value.state === "lower_bound" ? "At least " : ""}${value.value.bytes} bytes`;
}
function availability(row: FolderQueryRow): string {
  const flags = [row.flags.unenumerated, row.flags.excluded];
  const found: string[] = [];
  if (flags[0]?.state === "present" && flags[0].value)
    found.push("Not enumerated");
  if (flags[1]?.state === "present" && flags[1].value) found.push("Excluded");
  if (flags.some((flag) => flag.state === "unknown"))
    found.push("Availability unknown");
  return found.join(" · ") || "Not reported";
}
function rowKey(item: ItemRef): string {
  return `${item.resultVersion}:${item.itemNo}`;
}

type ParentLocation = { item?: ItemRef };

function parentLocation(
  opened: Opened,
  knownVirtualRoot: boolean,
): ParentLocation | undefined {
  const { rootKind, location, breadcrumbs, page } = opened;
  if (!location || location.resultVersion !== page.resultVersion)
    return undefined;
  if (rootKind === "scope_root") return knownVirtualRoot ? {} : undefined;
  if (rootKind !== "folder") return undefined;
  const current = breadcrumbs.at(-1)?.item;
  const parent = breadcrumbs.at(-2)?.item;
  if (!current || rowKey(current) !== rowKey(location)) return undefined;
  return parent?.resultVersion === page.resultVersion
    ? { item: parent }
    : undefined;
}

/** Isolated folder browser; its host supplies the transport and pinned result. */
export function BrowsePanel({
  port,
  resultVersion,
}: {
  port: BrowsePort;
  resultVersion?: string;
}) {
  const [view, setView] = useState<View>();
  const [busy, setBusy] = useState(false);
  const [loadingPage, setLoadingPage] = useState(false);
  const [message, setMessage] = useState<"error" | "job" | "page-error">();
  const sequence = useRef(0);
  const opening = useRef<AbortController | undefined>(undefined);
  const pageActive = useRef<PageViewport>(undefined);
  const pageViewport = useRef<PageViewport>(undefined);
  const virtualRootVersion = useRef<string>(undefined);
  const panel = useRef<HTMLElement>(null);
  const loadMoreButton = useRef<HTMLButtonElement>(null);
  const focusAfterUpdate = useRef<"navigation" | "page-end">(undefined);
  const current = useRef<View | undefined>(undefined);
  current.current = view;

  const recordLayoutScroll = useCallback((before: number, after: number) => {
    const viewport = pageViewport.current;
    // Only compensate for the movement produced by this layout operation.
    if (viewport && !viewport.scrolled && viewport.scrollY === before) {
      viewport.scrollY = after;
    }
  }, []);

  function navigate(location?: ItemRef, pinned = resultVersion) {
    const requestSeq = ++sequence.current;
    const active = document.activeElement;
    const restoreFocus =
      active !== panel.current && panel.current?.contains(active);
    opening.current?.abort();
    const controller = new AbortController();
    opening.current = controller;
    pageActive.current = undefined;
    pageViewport.current = undefined;
    setLoadingPage(false);
    setBusy(true);
    setMessage(undefined);
    void port
      .open(
        {
          viewKind: "list",
          requestSeq,
          ...(pinned ? { resultVersion: pinned } : {}),
          ...(location ? { location } : {}),
          pageSize: 200,
        },
        controller.signal,
      )
      .then((response) => {
        if (sequence.current !== requestSeq) return;
        opening.current = undefined;
        setBusy(false);
        if (response.requestSeq !== requestSeq) return;
        if (response.kind === "job") {
          setMessage("job");
          return;
        }
        if (pinned && response.page.resultVersion !== pinned) return;
        if (response.rootKind === "virtual_root") {
          virtualRootVersion.current = response.page.resultVersion;
        }
        focusAfterUpdate.current =
          restoreFocus && document.activeElement === active
            ? "navigation"
            : undefined;
        setView({
          opened: response,
          rows: response.page.rows,
          nextCursor: response.page.nextCursor,
        });
      })
      .catch(() => {
        if (sequence.current !== requestSeq) return;
        opening.current = undefined;
        setBusy(false);
        setMessage("error");
      });
  }

  useEffect(() => {
    virtualRootVersion.current = undefined;
    navigate(undefined, resultVersion);
    return () => {
      sequence.current += 1;
      opening.current?.abort();
    };
    // The injected port and requested result identify a new browser session.
  }, [port, resultVersion]);

  useLayoutEffect(() => {
    const focus = focusAfterUpdate.current;
    focusAfterUpdate.current = undefined;
    if (focus === "navigation") panel.current?.focus();
    if (focus === "page-end" && document.activeElement === document.body) {
      panel.current?.focus({ preventScroll: true });
    }
  }, [view]);

  useEffect(() => {
    const request = pageViewport.current;
    if (!loadingPage || !request) return;
    const trackScroll = () => {
      if (window.scrollY !== request.scrollY) request.scrolled = true;
    };
    window.addEventListener("scroll", trackScroll, { passive: true });
    return () => {
      window.removeEventListener("scroll", trackScroll);
      if (pageViewport.current === request) pageViewport.current = undefined;
    };
  }, [loadingPage]);

  function loadMore(keyboard: boolean) {
    const snapshot = current.current;
    if (!snapshot?.nextCursor || pageActive.current || busy) return;
    const cursor = snapshot.nextCursor;
    const requestSeq = sequence.current;
    const request = { scrollY: window.scrollY, scrolled: false };
    pageActive.current = request;
    pageViewport.current = request;
    setLoadingPage(true);
    setMessage(undefined);
    void port
      .page({ nextCursor: cursor, count: 200 })
      .then((page) => {
        if (sequence.current !== requestSeq) return;
        const active = current.current;
        if (!active || active.nextCursor !== cursor) return;
        if (
          page.rowKind !== "folder" ||
          page.resultVersion !== active.opened.page.resultVersion ||
          page.queryRef !== active.opened.page.queryRef
        ) {
          setMessage("page-error");
          return;
        }
        const known = new Set(active.rows.map((row) => rowKey(row.item)));
        if (page.rows.some((row) => known.has(rowKey(row.item)))) {
          setMessage("page-error");
          return;
        }
        const buttonFocused = document.activeElement === loadMoreButton.current;
        const focusNewRow =
          keyboard &&
          buttonFocused &&
          page.rows.length > 0 &&
          !request.scrolled &&
          window.scrollY === request.scrollY;
        focusAfterUpdate.current =
          buttonFocused && page.nextCursor === null && !focusNewRow
            ? "page-end"
            : undefined;
        setView({
          ...active,
          rows: [...active.rows, ...page.rows],
          nextCursor: page.nextCursor,
          pageFocus: focusNewRow
            ? {
                index: active.rows.length,
                viewport: request,
                button: loadMoreButton,
              }
            : undefined,
        });
      })
      .catch(() => {
        if (sequence.current === requestSeq) setMessage("page-error");
      })
      .finally(() => {
        if (sequence.current === requestSeq) {
          pageActive.current = undefined;
          setLoadingPage(false);
        }
      });
  }

  const page: FolderQueryPage | undefined = view?.opened.page;
  const hasVirtualRoot =
    page !== undefined && virtualRootVersion.current === page.resultVersion;
  const parent = view && parentLocation(view.opened, hasVirtualRoot);

  function goUp() {
    if (parent && page && !opening.current)
      navigate(parent.item, page.resultVersion);
  }

  function handleBrowseKey(event: KeyboardEvent<HTMLElement>) {
    if (
      event.key !== "ArrowUp" ||
      !event.altKey ||
      !parent ||
      !canHandleBrowseKey(event)
    )
      return;
    event.preventDefault();
    if (!event.repeat) goUp();
  }

  return (
    <section
      ref={panel}
      tabIndex={-1}
      className="du-results du-browse-panel"
      aria-label="Folder navigation"
      onKeyDown={handleBrowseKey}
    >
      <Heading level={2}>Folders and files</Heading>
      {busy && <Progress label="Opening folder" />}
      {message === "error" && (
        <Notice tone="error">Could not open this folder.</Notice>
      )}
      {message === "page-error" && (
        <Notice tone="error">Could not load the next page.</Notice>
      )}
      {message === "job" && <Notice>Folder results are being prepared.</Notice>}
      {view && page && (
        <>
          <Text kind="muted">
            Source: {view.opened.origin} · Observed:{" "}
            {page.observedAt ?? "Unavailable"}
          </Text>
          <div className="du-card__actions" aria-label="Folder path">
            <Action onPress={goUp} isDisabled={!parent}>
              Up
            </Action>
            {view.opened.rootKind === "virtual_root" ? (
              <Text>Scope roots</Text>
            ) : (
              hasVirtualRoot && (
                <Action onPress={() => navigate(undefined, page.resultVersion)}>
                  Scope roots
                </Action>
              )
            )}
            {view.opened.breadcrumbs.map((crumb) => (
              <Action
                key={rowKey(crumb.item)}
                onPress={() => navigate(crumb.item, page.resultVersion)}
              >
                {name(crumb.displayName)}
              </Action>
            ))}
          </div>
          {page.completeness !== "complete" && (
            <Notice>
              Results are incomplete; some items may be unavailable.
            </Notice>
          )}
          <Text kind="muted">
            {page.total.state === "lower_bound" ? "At least " : ""}
            {page.total.count} items
          </Text>
          {page.unknownCount > 0 && (
            <Text kind="muted">
              {page.unknownCount} items could not be evaluated.
            </Text>
          )}
          {view.rows.length === 0 && (
            <Text>No observed items in this folder.</Text>
          )}
          <BrowseTable
            key={view.opened.requestSeq}
            label="Folder items"
            columns={[
              "Name",
              "Type",
              "Logical size",
              "Allocated size",
              "Modified",
              "Availability",
            ]}
            rows={view.rows}
            pageFocus={view.pageFocus}
            scrollFooter={loadMoreButton}
            onLayoutScroll={recordLayoutScroll}
            getRowKey={(row) => rowKey(row.item)}
            totalRows={page.total.state === "exact" ? page.total.count : -1}
            renderCells={(row) => [
              row.kind === "directory" ? (
                <Action onPress={() => navigate(row.item, page.resultVersion)}>
                  {name(row.displayName)}
                </Action>
              ) : (
                <Text>{name(row.displayName)}</Text>
              ),
              <Text>
                {row.kind === "unknown" || row.kind === "unknown_structure"
                  ? "Unavailable"
                  : row.kind}
              </Text>,
              <Text>{size(row.logicalSize)}</Text>,
              <Text>{size(row.allocatedSize)}</Text>,
              <Text>
                {row.modifiedAt.state === "present"
                  ? row.modifiedAt.value
                  : row.modifiedAt.state === "absent"
                    ? "Not recorded"
                    : "Unavailable"}
              </Text>,
              <Text>{availability(row)}</Text>,
            ]}
          />
          {view.nextCursor && (
            <div className="du-browse-more">
              <Action
                ref={loadMoreButton}
                onPress={(event) =>
                  loadMore(
                    event.pointerType === "keyboard" ||
                      event.pointerType === "virtual",
                  )
                }
                isDisabled={busy}
              >
                {loadingPage ? "Loading…" : "Load more"}
              </Action>
            </div>
          )}
        </>
      )}
    </section>
  );
}
