import { useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import {
  Action,
  BrowseGrid,
  ItemActions,
  ActionFeedback,
  Moment,
  NameText,
  Notice,
  RowAction,
  Text,
  Unbroken,
} from "../../components";
import type { BrowseGridFocus, ItemAction } from "../../components";
import type {
  FolderQueryRow,
  BrowseCopyPathOutcome,
  ItemRef,
  QueryPageRequest,
  QueryPageResponse,
} from "../../ipc";
import type { BrowsePages } from "../../state/browse-pages";
import type { ScanPriority } from "../../state/scan-priority";
import { browseDisplayName, rowKey } from "./browse-display";
import { formatByteUnit } from "../../i18n/bytes";
import type { SizeDisplay } from "../../i18n/bytes";
import { useTimestampParts } from "../../i18n/regional-format";
import { useLocaleText } from "../../i18n/presentation";
import { SIZE_UNIT_RULES } from "../../ipc";
import { useBrowsePaging } from "./use-browse-paging";
import { useBrowseCopy } from "./use-browse-copy";
import {
  canAnalyzeFirst,
  newerResultReason,
  SelectionAnalyzeFirst,
  waitsForNewerResult,
} from "./SelectionAnalyzeFirst";
import type { Selection } from "./SelectionAnalyzeFirst";

/**
 * Focuses the list's entry after a new folder replaced it: its first row, or
 * for an empty folder the "Up" action. `origin` is the control that held
 * focus when the folder was opened.
 */
export type ListEntryFocus = (origin: Element) => void;

function size(
  value: FolderQueryRow["logicalSize"],
  display: SizeDisplay,
  text: ReturnType<typeof useLocaleText>,
): string {
  if (value.state === "absent")
    return text("ui.browse.not_recorded", "Not recorded");
  if (value.state !== "present" || value.value.state === "unknown")
    return text("ui.browse.unavailable", "Unavailable");
  const lowerBound = value.value.state === "lower_bound";
  const readable = formatByteUnit(
    BigInt(value.value.bytes),
    SIZE_UNIT_RULES[display.system],
    SIZE_UNIT_RULES.decimalPlaces,
    display.unit,
    lowerBound,
    display.regional,
  );
  return `${lowerBound ? `${text("ui.browse.at_least", "At least")} ` : ""}${readable}`;
}

function availability(
  row: FolderQueryRow,
  text: ReturnType<typeof useLocaleText>,
): string {
  const flags = [row.flags.unenumerated, row.flags.excluded];
  const found: string[] = [];
  if (flags[0]?.state === "present" && flags[0].value)
    found.push(text("ui.browse.not_enumerated", "Not enumerated"));
  if (flags[1]?.state === "present" && flags[1].value)
    found.push(text("ui.browse.excluded", "Excluded"));
  if (flags.some((flag) => flag.state === "unknown"))
    found.push(text("ui.browse.availability_unknown", "Unknown"));
  return found.join(" · ") || text("ui.browse.not_reported", "Not reported");
}

/**
 * "Analyze first" in the row menu: available for a folder of the running
 * scan's current result, and unavailable with its reason while the list shows
 * an earlier result of that scan.
 */
function analyzeFirstActions(
  row: FolderQueryRow,
  priority: ScanPriority | undefined,
  readOnly: boolean,
  text: ReturnType<typeof useLocaleText>,
): ItemAction[] {
  const label = text("ui.browse.analyze_first", "Analyze first");
  if (waitsForNewerResult(row, priority, readOnly))
    return [
      {
        id: "analyze-first",
        label,
        onAction: () => undefined,
        disabledReason: newerResultReason(text),
      },
    ];
  if (!canAnalyzeFirst(row, priority, readOnly)) return [];
  return [
    {
      id: "analyze-first",
      label,
      onAction: () =>
        priority.request(row.item, browseDisplayName(row.displayName, text)),
    },
  ];
}

/** The row's collection status; a requested folder shows that instead. */
function StatusCell({
  row,
  priority,
  readOnly,
}: {
  row: FolderQueryRow;
  priority: ScanPriority | undefined;
  readOnly: boolean;
}) {
  const text = useLocaleText();
  const status =
    row.kind === "directory" ? priority?.status(row.item) : undefined;
  return (
    <div className="du-row-status" data-requested={status ? "" : undefined}>
      <Text>
        {status === "analyzing"
          ? text("ui.browse.priority_analyzing", "Analyzing first")
          : status === "requested"
            ? text("ui.browse.priority_requested", "Analyze first requested")
            : availability(row, text)}
      </Text>
      {canAnalyzeFirst(row, priority, readOnly) && (
        <RowAction
          label={text("ui.browse.analyze_first", "Analyze first")}
          onPress={() =>
            priority.request(row.item, browseDisplayName(row.displayName, text))
          }
        />
      )}
    </div>
  );
}

/** The disabled reason of the copy action: the host's reason first, else "Unavailable now". */
function copyDisabled(
  hostReason: string | undefined,
  unavailable: boolean,
  text: ReturnType<typeof useLocaleText>,
): { disabledReason?: string } {
  if (hostReason !== undefined) return { disabledReason: hostReason };
  if (!unavailable) return {};
  return {
    disabledReason: text("ui.browse.unavailable_now", "Unavailable now"),
  };
}

export function BrowseResults({
  pages,
  display,
  initialCursor,
  readPage,
  busy,
  readOnly,
  navigationEpoch,
  navigationSeq,
  canRead,
  focusPanel,
  focusUp,
  listEntry,
  barFocus,
  openFailures,
  retryOpen,
  openFolder,
  openItem,
  copyPath,
  copyDisabledReason,
  reopen,
  priority,
}: {
  pages: BrowsePages;
  display: SizeDisplay;
  initialCursor: string | null;
  readPage: (request: QueryPageRequest) => Promise<QueryPageResponse>;
  busy: boolean;
  readOnly: boolean;
  navigationEpoch: number;
  navigationSeq: () => number;
  canRead: () => boolean;
  /**
   * Focuses the panel. Used only where no nearer next step exists: an empty
   * folder without an available "Up", a final page that added no rows, and a
   * successful page retry.
   */
  focusPanel: () => void;
  /** Focuses "Up" when it is available; false when it is not. */
  focusUp: () => boolean;
  /** Receives this list's entry focus, which the panel calls after opening it. */
  listEntry: RefObject<ListEntryFocus | undefined>;
  /** Whether the selection action bar holds focus; outlives this list. */
  barFocus: RefObject<boolean>;
  /** Counts failed openings of a new list; see {@link SelectionAnalyzeFirst}. */
  openFailures: number;
  /** Opens the list the panel failed to open, again. */
  retryOpen: () => void;
  openFolder: (item: ItemRef) => void;
  openItem:
    ((row: FolderQueryRow, trigger: Element | null) => void) | undefined;
  copyPath: ((item: ItemRef) => Promise<BrowseCopyPathOutcome>) | undefined;
  /** Why copying the path is off for every row, when the host turned it off. */
  copyDisabledReason: string | undefined;
  reopen: () => void;
  priority?: ScanPriority | undefined;
}) {
  const text = useLocaleText();
  const timestamp = useTimestampParts();
  const columns = [
    text("ui.browse.column_name", "Name"),
    text("ui.browse.column_type", "Type"),
    text("ui.browse.column_logical_size", "Logical size"),
    text("ui.browse.column_allocated_size", "Allocated size"),
    text("ui.browse.column_modified", "Modified"),
    text("ui.browse.column_availability", "Availability"),
  ];
  const scrollRoot = useRef<HTMLDivElement>(null);
  const [selection, setSelection] = useState<Selection>(() => new Set());
  const [rowFocus, setRowFocus] = useState<BrowseGridFocus>();
  const copy = useBrowseCopy(
    copyPath,
    navigationEpoch,
    navigationSeq,
    scrollRoot,
  );
  const paging = useBrowsePaging({
    scrollRoot,
    pages,
    initialCursor,
    readPage,
    enabled: !busy,
    navigationEpoch,
    navigationSeq,
    canRead,
    focusPanel,
  });
  const loadHasFocus = useRef(false);
  useLayoutEffect(() => {
    function focusIn(event: FocusEvent) {
      loadHasFocus.current = event.target === paging.loadButton.current;
    }
    function focusOut(event: FocusEvent) {
      if (event.target === paging.loadButton.current)
        loadHasFocus.current = false;
    }
    document.addEventListener("focusin", focusIn);
    document.addEventListener("focusout", focusOut);
    return () => {
      document.removeEventListener("focusin", focusIn);
      document.removeEventListener("focusout", focusOut);
    };
  }, [paging.loadButton]);
  useLayoutEffect(() => {
    if (
      paging.failure &&
      loadHasFocus.current &&
      document.activeElement === document.body
    ) {
      loadHasFocus.current = false;
      paging.retryButton.current?.focus({ preventScroll: true });
    }
  }, [paging.failure, paging.retryButton]);
  /**
   * Focuses a row once it is drawn, unless `eligible` says focus has moved
   * on by then.
   */
  function requestRow(index: number, eligible: () => boolean) {
    const request: BrowseGridFocus = {
      index,
      eligible,
      finish: () =>
        setRowFocus((current) => (current === request ? undefined : current)),
    };
    setRowFocus(request);
  }
  /** An empty folder's next step is "Up"; without it, the panel. */
  function focusEmpty() {
    if (!focusUp()) focusPanel();
  }
  /**
   * Focuses a row, the element keyboard navigation of the list focuses: the
   * first selected row shown, else the first row shown. A list that has not
   * drawn its rows yet focuses its first row once drawn, unless focus has
   * moved elsewhere than the panel by then. An empty list focuses "Up".
   */
  function focusList() {
    const rows = scrollRoot.current;
    const row =
      rows?.querySelector<HTMLElement>('[data-row][aria-selected="true"]') ??
      rows?.querySelector<HTMLElement>("[data-row]");
    if (row) {
      row.focus({ preventScroll: true });
      return;
    }
    if (pages.count === 0) {
      focusEmpty();
      return;
    }
    requestRow(0, () => {
      const active = document.activeElement;
      return (
        active === null ||
        active === document.body ||
        (active !== scrollRoot.current && active.contains(scrollRoot.current))
      );
    });
  }
  /**
   * The entry of a newly opened folder: its first row, or "Up" when it is
   * empty. The first row is focused once drawn while focus still rests on the
   * control that opened the folder, or was lost with it.
   */
  function focusEntry(origin: Element) {
    if (pages.count === 0) {
      focusEmpty();
      return;
    }
    const row =
      scrollRoot.current?.querySelector<HTMLElement>('[data-row="0"]');
    if (row) {
      row.focus({ preventScroll: true });
      return;
    }
    requestRow(0, () => {
      const active = document.activeElement;
      return active === null || active === document.body || active === origin;
    });
  }
  useLayoutEffect(() => {
    listEntry.current = focusEntry;
  });
  return (
    <div className="du-browse-results">
      <div className="du-browse-controls">
        <SelectionAnalyzeFirst
          pages={pages}
          selection={selection}
          priority={priority}
          readOnly={readOnly}
          focusList={focusList}
          barFocus={barFocus}
          openFailures={openFailures}
          retryOpen={retryOpen}
        />
        {pages.count === 0 && (
          <Text>
            {text("ui.browse.no_items", "No observed items in this folder.")}
          </Text>
        )}
        {paging.failure && (
          <Notice tone="error">
            {paging.failure.stale
              ? text(
                  "ui.browse.stale_page",
                  "This result can no longer be read. Reopen this folder to continue.",
                )
              : text(
                  "ui.browse.page_failed",
                  "Could not load the requested page.",
                )}{" "}
            <Action
              ref={paging.retryButton}
              isDisabled={busy && !paging.failure.stale}
              isPending={paging.retrying || (busy && paging.failure.stale)}
              onPress={(event) => {
                if (paging.failure?.stale) reopen();
                else
                  paging.retry(
                    event.pointerType === "keyboard" ||
                      event.pointerType === "virtual",
                  );
              }}
            >
              {paging.failure.stale
                ? busy
                  ? text("ui.browse.reopening_folder", "Reopening folder…")
                  : text("ui.browse.reopen_folder", "Reopen folder")
                : paging.retrying
                  ? text("ui.browse.retrying_page", "Retrying page…")
                  : text("ui.browse.retry_page", "Retry page")}
            </Action>
          </Notice>
        )}
        {initialCursor !== null && (
          <div className="du-browse-more">
            {paging.showLoadMore && !paging.failure && (
              <Action
                ref={paging.loadButton}
                onPress={(event) =>
                  paging.loadMore(
                    event.pointerType === "keyboard" ||
                      event.pointerType === "virtual",
                  )
                }
                isDisabled={busy}
              >
                {paging.loadingMore
                  ? text("ui.browse.loading", "Loading…")
                  : text("ui.browse.load_more", "Load more")}
              </Action>
            )}
          </div>
        )}
      </div>
      <BrowseGrid<FolderQueryRow>
        scrollRoot={scrollRoot}
        label={text("ui.browse.folder_items", "Folder items")}
        columns={columns}
        count={pages.count}
        rowCount={pages.rowCount}
        busy={paging.pending}
        focusRequest={paging.focusRequest ?? rowFocus}
        onMove={paging.move}
        onOpen={(row) => {
          if (readOnly) return;
          if (row.kind === "directory") openFolder(row.item);
          if (row.kind === "file") openItem?.(row, document.activeElement);
        }}
        onFocusRow={paging.trackFocus}
        onVisible={paging.show}
        onLayoutScroll={paging.recordLayoutScroll}
        getRow={(index) => pages.row(index)}
        getKey={(row) => rowKey(row.item)}
        onSelectionChange={setSelection}
        renderCells={(row) => [
          <ItemActions
            rowScope
            label={text("ui.browse.item_actions", "Actions for {name}", {
              name: browseDisplayName(row.displayName, text),
            })}
            actions={[
              ...((row.kind === "file" || row.kind === "directory") && openItem
                ? [
                    {
                      id: "inspect",
                      label: text("ui.browse.inspect", "Inspect"),
                      onAction: (trigger: HTMLElement | null) =>
                        openItem(row, trigger),
                      ...(busy
                        ? {
                            disabledReason: text(
                              "ui.browse.unavailable_while_opening",
                              "Unavailable while opening",
                            ),
                          }
                        : {}),
                    },
                  ]
                : []),
              ...analyzeFirstActions(row, priority, readOnly, text),
              {
                id: "copy",
                label: text("ui.browse.copy_path", "Copy path"),
                onAction: (trigger: HTMLElement | null) =>
                  copy.copy(row, trigger),
                ...copyDisabled(
                  copyDisabledReason,
                  !copyPath || busy || copy.busy,
                  text,
                ),
              },
            ]}
          >
            {row.kind === "directory" ? (
              <Action
                excludeFromTabOrder
                isDisabled={readOnly}
                onPress={() => openFolder(row.item)}
              >
                <NameText name={browseDisplayName(row.displayName, text)} />
              </Action>
            ) : row.kind === "file" && openItem ? (
              <Action
                excludeFromTabOrder
                isDisabled={busy}
                onPress={(event) => openItem(row, event.target)}
              >
                <NameText name={browseDisplayName(row.displayName, text)} />
              </Action>
            ) : (
              <Text>
                <NameText name={browseDisplayName(row.displayName, text)} />
              </Text>
            )}
          </ItemActions>,
          <Text>
            {row.kind === "unknown" || row.kind === "unknown_structure"
              ? text("ui.browse.type_unavailable", "Unavailable")
              : row.kind === "directory"
                ? text("ui.browse.type_directory", "Folder")
                : row.kind === "file"
                  ? text("ui.browse.type_file", "File")
                  : row.kind === "untraversed_link"
                    ? text(
                        "ui.browse.type_untraversed_link",
                        "Untraversed link",
                      )
                    : text("ui.browse.type_unavailable", "Unavailable")}
          </Text>,
          <Text>
            <Unbroken>{size(row.logicalSize, display, text)}</Unbroken>
          </Text>,
          <Text>
            <Unbroken>{size(row.allocatedSize, display, text)}</Unbroken>
          </Text>,
          <Text>
            {row.modifiedAt.state === "present"
              ? (() => {
                  const modified = row.modifiedAt.value;
                  return (
                    <Moment
                      {...timestamp(Date.parse(modified), () => modified)}
                    />
                  );
                })()
              : row.modifiedAt.state === "absent"
                ? text("ui.browse.not_recorded", "Not recorded")
                : text("ui.browse.unavailable", "Unavailable")}
          </Text>,
          <StatusCell row={row} priority={priority} readOnly={readOnly} />,
        ]}
      />
      <ActionFeedback
        status={
          copy.status ||
          (paging.moving
            ? text("ui.browse.loading_folder_items", "Loading folder items…")
            : paging.status) ||
          ""
        }
        parts={copy.feedback}
        pending={copy.busy || !copy.status}
        onDismiss={copy.dismiss}
      />
    </div>
  );
}
