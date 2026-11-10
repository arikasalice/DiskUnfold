import { useEffect, useRef, useState } from "react";
import {
  Action,
  DataTable,
  Heading,
  Notice,
  Progress,
  Text,
} from "../../components";
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
  const pageActive = useRef(false);
  const hasVirtualRoot = useRef(false);
  const current = useRef<View | undefined>(undefined);
  current.current = view;

  function navigate(location?: ItemRef, pinned = resultVersion) {
    const requestSeq = ++sequence.current;
    opening.current?.abort();
    const controller = new AbortController();
    opening.current = controller;
    pageActive.current = false;
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
        setBusy(false);
        if (response.requestSeq !== requestSeq) return;
        if (response.kind === "job") {
          setMessage("job");
          return;
        }
        if (pinned && response.page.resultVersion !== pinned) return;
        if (response.rootKind === "virtual_root") hasVirtualRoot.current = true;
        setView({
          opened: response,
          rows: response.page.rows,
          nextCursor: response.page.nextCursor,
        });
      })
      .catch(() => {
        if (sequence.current !== requestSeq) return;
        setBusy(false);
        setMessage("error");
      });
  }

  useEffect(() => {
    hasVirtualRoot.current = false;
    navigate(undefined, resultVersion);
    return () => {
      sequence.current += 1;
      opening.current?.abort();
    };
    // The injected port and requested result identify a new browser session.
  }, [port, resultVersion]);

  function loadMore() {
    const snapshot = current.current;
    if (!snapshot?.nextCursor || pageActive.current || busy) return;
    const cursor = snapshot.nextCursor;
    const requestSeq = sequence.current;
    pageActive.current = true;
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
        setView({
          ...active,
          rows: [...active.rows, ...page.rows],
          nextCursor: page.nextCursor,
        });
      })
      .catch(() => {
        if (sequence.current === requestSeq) setMessage("page-error");
      })
      .finally(() => {
        if (sequence.current === requestSeq) {
          pageActive.current = false;
          setLoadingPage(false);
        }
      });
  }

  const page: FolderQueryPage | undefined = view?.opened.page;
  return (
    <section className="du-results" aria-label="Folder navigation">
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
            {view.opened.rootKind === "virtual_root" ? (
              <Text>Scope roots</Text>
            ) : (
              hasVirtualRoot.current && (
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
          <DataTable
            density="compact"
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
            <Action onPress={loadMore} isDisabled={loadingPage || busy}>
              {loadingPage ? "Loading…" : "Load more"}
            </Action>
          )}
        </>
      )}
    </section>
  );
}
