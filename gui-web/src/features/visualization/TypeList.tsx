import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import {
  Action,
  Heading,
  MiddlePath,
  Moment,
  NameText,
  Notice,
  SortedTable,
  Text,
} from "../../components";
import type { SortedColumn, TableSort } from "../../components";
import type {
  BrowseSortKey,
  BrowseValue,
  Id,
  ItemRef,
  QueryCondition,
  SearchQueryPage,
  SearchQueryRow,
  SizeBasis,
  SizeValue,
  VizQuantity,
} from "../../ipc";
import { useTimestampParts } from "../../i18n/regional-format";
import { useLocaleText } from "../../i18n/presentation";
import { vizMessage } from "../../viz";
import type { VizText } from "../../viz";
import type { TypeListSummary } from "./ModelLegend";

export type TypeListRequest = {
  resultVersion: Id;
  /** The folder the legend counts; the list holds the files below it. */
  scope: ItemRef;
  condition: QueryCondition;
  sortKeys: BrowseSortKey[];
};

/** Opens the list and reads on (the app's query.run port, F04 IF-01). */
export type TypeListSource = {
  open(
    request: TypeListRequest,
    signal: AbortSignal,
  ): Promise<{ page: SearchQueryPage; scopePath: string | null }>;
  next(cursor: string): Promise<SearchQueryPage>;
};

/** Below this list width the date goes and the location moves under the name. */
const NARROW_WIDTH = 720;
/** "Building the list…" appears only when the list takes longer (README 46.13). */
const BUILDING_DELAY_MS = 300;

type Ready = {
  kind: "ready";
  rows: SearchQueryRow[];
  next: string | null;
  scopePath: string | null;
  more: "idle" | "loading" | "failed";
};
type ListState =
  { kind: "loading"; building: boolean } | { kind: "failed" } | Ready;

type Props = {
  source: TypeListSource;
  resultVersion: Id;
  scope: ItemRef;
  condition: QueryCondition;
  type: string;
  summary: TypeListSummary;
  text: VizText;
  /** Back to the chart, narrowed as before. */
  onBack: () => void;
  /** The chip's ×: clears the type and returns to the whole chart. */
  onClear: () => void;
};

/**
 * The files of the chosen type below the chart's folder, in the chart's place
 * (F06 README 46.13, #306): "File list", the type chip and the legend's values,
 * then a table sorted by size, largest first.
 */
export function TypeList(props: Props) {
  const { text, summary } = props;
  const title = useRef<HTMLHeadingElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const narrow = useNarrow(root);
  const [sort, setSort] = useState<TableSort>({
    column: "size",
    direction: "descending",
  });
  const list = useListState(props, sort);
  useLayoutEffect(() => {
    title.current?.focus({ preventScroll: true });
  }, []);
  const count = countText(summary.count, text);
  return (
    <div ref={root} className="du-type-list">
      <div className="du-type-list__head">
        <Action variant="quiet" icon="back" onPress={props.onBack}>
          {text.message("listBack")}
        </Action>
        <Heading level={3} ref={title} programmaticFocus>
          {text.message("listTitle")}
        </Heading>
        <div className="du-type-list__condition">
          <Action
            onPress={props.onClear}
            aria-label={text.message("filterClearType")}
          >
            {`${vizMessage(text, "legendChip", { criterion: text.message("colorType"), name: props.type })} ×`}
          </Action>
          <Text kind="muted">
            {vizMessage(text, "listTotal", {
              type: props.type,
              count,
              size: text.quantity(summary.size),
              basis: text.key(summary.sizeBasis),
            })}
          </Text>
        </div>
      </div>
      <div className="du-visually-hidden" role="status" aria-live="polite">
        {list.state.kind === "ready"
          ? vizMessage(text, "listAnnounce", { type: props.type, count })
          : ""}
      </div>
      <ListBody
        {...props}
        list={list}
        narrow={narrow}
        sort={sort}
        onSort={setSort}
      />
    </div>
  );
}

function ListBody({
  list,
  narrow,
  sort,
  onSort,
  text,
  type,
  summary,
  onBack,
}: Props & {
  list: ReturnType<typeof useListState>;
  narrow: boolean;
  sort: TableSort;
  onSort: (sort: TableSort) => void;
}) {
  const { state } = list;
  if (state.kind === "loading")
    return state.building ? (
      <div className="du-type-list__building">
        <Text>{text.message("listBuilding")}</Text>
        <Action onPress={onBack}>{text.message("listCancel")}</Action>
      </div>
    ) : null;
  if (state.kind === "failed")
    return (
      <Notice tone="error">
        {text.message("listFailed")}{" "}
        <Action onPress={list.retry}>{text.message("listRetry")}</Action>
      </Notice>
    );
  if (state.rows.length === 0)
    return <Text>{vizMessage(text, "listEmpty", { type })}</Text>;
  return (
    <ListTable
      state={state}
      narrow={narrow}
      sort={sort}
      onSort={onSort}
      text={text}
      sizeBasis={summary.sizeBasis}
      loadMore={list.loadMore}
    />
  );
}

function ListTable({
  state,
  narrow,
  sort,
  onSort,
  text,
  sizeBasis,
  loadMore,
}: {
  state: Ready;
  narrow: boolean;
  sort: TableSort;
  onSort: (sort: TableSort) => void;
  text: VizText;
  sizeBasis: SizeBasis;
  loadMore: () => void;
}) {
  const locale = useLocaleText();
  const timestamp = useTimestampParts();
  const columns: SortedColumn[] = [
    { id: "name", label: text.message("listColumnName"), sortable: true },
    ...(narrow
      ? []
      : [
          {
            id: "location",
            label: text.message("listColumnLocation"),
            sortable: true,
          },
        ]),
    {
      id: "size",
      label: text.message("listColumnSize"),
      sortable: true,
      firstDirection: "descending",
    },
    ...(narrow
      ? []
      : [{ id: "modified", label: text.message("listColumnModified") }]),
  ];
  const unavailable = (value: BrowseValue<unknown>) =>
    value.state === "absent"
      ? locale("ui.browse.not_recorded", "Not recorded")
      : locale("ui.browse.unavailable", "Unavailable");
  const name = (row: SearchQueryRow) =>
    row.displayName.state === "present"
      ? row.displayName.value
      : unavailable(row.displayName);
  const location = (row: SearchQueryRow) =>
    row.displayPath.state === "present"
      ? locationOf(row.displayPath.value, state.scopePath)
      : unavailable(row.displayPath);
  return (
    <SortedTable
      label={text.message("listTitle")}
      columns={columns}
      rows={state.rows}
      getKey={(row) => `${row.item.resultVersion}:${row.item.itemNo}`}
      sort={sort}
      onSort={onSort}
      renderCells={(row) => [
        <div className="du-type-list__name">
          <NameText name={name(row)} />
          {narrow && <MiddlePath path={location(row)} />}
        </div>,
        ...(narrow ? [] : [<MiddlePath path={location(row)} />]),
        <Text>{sizeText(row, sizeBasis, text, unavailable)}</Text>,
        ...(narrow
          ? []
          : [
              <Text>
                {row.modifiedAt.state === "present" ? (
                  <Moment
                    {...timestamp(Date.parse(row.modifiedAt.value), () =>
                      row.modifiedAt.state === "present"
                        ? row.modifiedAt.value
                        : "",
                    )}
                  />
                ) : (
                  unavailable(row.modifiedAt)
                )}
              </Text>,
            ]),
      ]}
      footer={<MoreRows state={state} text={text} loadMore={loadMore} />}
    />
  );
}

/** Reads on when the end of the table scrolls into view, or on the button. */
function MoreRows({
  state,
  text,
  loadMore,
}: {
  state: Ready;
  text: VizText;
  loadMore: () => void;
}) {
  const sentinel = useRef<HTMLDivElement>(null);
  const hasNext = state.next !== null;
  useEffect(() => {
    const element = sentinel.current;
    if (!element || !hasNext || typeof IntersectionObserver === "undefined")
      return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) loadMore();
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [hasNext, loadMore]);
  if (!hasNext) return null;
  return (
    <div ref={sentinel} className="du-type-list__more">
      {state.more === "failed" && (
        <Text kind="muted">{text.message("listFailed")}</Text>
      )}
      <Action
        variant="quiet"
        isPending={state.more === "loading"}
        onPress={loadMore}
      >
        {state.more === "loading"
          ? text.message("listLoadingMore")
          : text.message("listLoadMore")}
      </Action>
    </div>
  );
}

/**
 * The parent folder of a file from the scope folder's name on (README 46.13):
 * "type-narrowing-scope\docs", or just the scope name for a file directly in
 * it. Display text only; when the scope path is unknown or does not lead the
 * file's path, the whole parent path.
 */
export function locationOf(path: string, scopePath: string | null): string {
  const parent = path.slice(0, Math.max(0, lastSeparator(path)));
  if (!scopePath) return parent;
  const cut = lastSeparator(scopePath);
  // A volume root such as "C:\" keeps its whole path as its name.
  if (cut < 0 || cut === scopePath.length - 1) return parent;
  const base = scopePath.slice(0, cut + 1);
  return parent.startsWith(base) ? parent.slice(base.length) : parent;
}

function lastSeparator(path: string) {
  return Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
}

function sizeText(
  row: SearchQueryRow,
  basis: SizeBasis,
  text: VizText,
  unavailable: (value: BrowseValue<unknown>) => string,
) {
  const value: BrowseValue<SizeValue> =
    basis === "allocated" ? row.allocatedSize : row.logicalSize;
  return value.state === "present"
    ? text.size(value.value)
    : unavailable(value);
}

/** "12 files", or "at least 12 files" when the legend's count is a lower bound. */
function countText(count: VizQuantity, text: VizText) {
  if (count.state === "unknown") return text.quantity(count);
  return vizMessage(
    text,
    count.state === "lower_bound" ? "listCountAtLeast" : "listCount",
    { count: BigInt(count.value) },
  );
}

function sortKey(sort: TableSort, basis: SizeBasis): BrowseSortKey {
  const descending = sort.direction === "descending";
  if (sort.column === "name")
    return { id: "sort.name", revision: 1, descending };
  if (sort.column === "location")
    return { id: "sort.path", revision: 1, descending };
  return {
    id: "sort.size",
    revision: 1,
    descending,
    ...(basis === "logical" || basis === "allocated"
      ? { sizeBasis: basis }
      : {}),
  };
}

/** The first page for the current sort, then the pages read on. */
function useListState(props: Props, sort: TableSort) {
  const { source, resultVersion, scope, condition, summary } = props;
  const [state, setState] = useState<ListState>({
    kind: "loading",
    building: false,
  });
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const abort = new AbortController();
    setState({ kind: "loading", building: false });
    const timer = setTimeout(
      () =>
        setState((current) =>
          current.kind === "loading"
            ? { kind: "loading", building: true }
            : current,
        ),
      BUILDING_DELAY_MS,
    );
    source
      .open(
        {
          resultVersion,
          scope,
          condition,
          sortKeys: [sortKey(sort, summary.sizeBasis)],
        },
        abort.signal,
      )
      .then(
        ({ page, scopePath }) => {
          if (abort.signal.aborted) return;
          setState({
            kind: "ready",
            rows: page.rows,
            next: page.nextCursor,
            scopePath,
            more: "idle",
          });
        },
        () => {
          if (!abort.signal.aborted) setState({ kind: "failed" });
        },
      )
      .finally(() => clearTimeout(timer));
    return () => {
      clearTimeout(timer);
      abort.abort();
    };
  }, [
    source,
    resultVersion,
    scope,
    condition,
    sort,
    summary.sizeBasis,
    attempt,
  ]);
  const loadMore = useLatest(() => {
    if (state.kind !== "ready" || !state.next || state.more === "loading")
      return;
    const cursor = state.next;
    const same = (current: ListState): current is Ready =>
      current.kind === "ready" && current.next === cursor;
    setState({ ...state, more: "loading" });
    source.next(cursor).then(
      (page) =>
        setState((current) =>
          same(current)
            ? {
                ...current,
                rows: [...current.rows, ...page.rows],
                next: page.nextCursor,
                more: "idle",
              }
            : current,
        ),
      () =>
        setState((current) =>
          same(current) ? { ...current, more: "failed" } : current,
        ),
    );
  });
  return { state, loadMore, retry: () => setAttempt((value) => value + 1) };
}

/** A stable callback that always runs the latest closure. */
function useLatest(callback: () => void) {
  const latest = useRef(callback);
  useLayoutEffect(() => {
    latest.current = callback;
  });
  const [stable] = useState(() => () => latest.current());
  return stable;
}

function useNarrow(root: RefObject<HTMLDivElement | null>) {
  const [narrow, setNarrow] = useState(false);
  useLayoutEffect(() => {
    const element = root.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width;
      if (width !== undefined && width > 0) setNarrow(width < NARROW_WIDTH);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [root]);
  return narrow;
}
