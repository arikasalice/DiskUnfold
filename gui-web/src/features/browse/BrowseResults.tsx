import { useId, useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import {
  Action,
  BrowseGrid,
  ItemActions,
  ActionFeedback,
  Moment,
  NameText,
  Notice,
  Text,
  Unbroken,
} from "../../components";
import type { BrowseGridFocus } from "../../components";
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
import {
  browseNotice,
  copyDisabled,
  notCounted,
  scanStatus,
  size,
} from "./browse-cells";
import type { SizeDisplay } from "../../i18n/bytes";
import { useTimestampParts } from "../../i18n/regional-format";
import { useLocaleText } from "../../i18n/presentation";
import { useBrowsePaging } from "./use-browse-paging";
import { useBrowseCopy } from "./use-browse-copy";
import { PriorityToast, SelectionAnalyzeFirst } from "./SelectionAnalyzeFirst";
import type { Selection } from "./SelectionAnalyzeFirst";
import {
  analyzeFirstActions,
  priorityBadge,
  PrioritySlot,
} from "./PrioritySlot";

function ResultsNotice({ lines }: { lines: string[] | undefined }) {
  if (!lines?.length) return null;
  return (
    <Notice>
      {lines.length === 1 ? (
        lines[0]
      ) : (
        <div className="du-browse-results__notice-lines">
          {lines.map((line, index) => (
            <Text key={index}>{line}</Text>
          ))}
        </div>
      )}
    </Notice>
  );
}

/**
 * Focuses the list's entry after a new folder replaced it: its first row, or
 * for an empty folder the "Up" action. `origin` is the control that held
 * focus when the folder was opened.
 */
export type ListEntryFocus = (origin: Element) => void;

/**
 * Focuses the row at an index once it is drawn, or the last row when the
 * list has fewer rows; an empty list focuses "Up", else the title. Used when
 * the row that held focus went away.
 */
export type ListRowFocus = (index: number) => void;

export function BrowseResults({
  pages,
  incomplete,
  dedupCapacityLowerBound,
  display,
  initialCursor,
  readPage,
  busy,
  readOnly,
  navigationEpoch,
  navigationSeq,
  canRead,
  focusTitle,
  focusUp,
  listEntry,
  listRow,
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
  incomplete: boolean;
  dedupCapacityLowerBound: boolean;
  display: SizeDisplay;
  initialCursor: string | null;
  readPage: (request: QueryPageRequest) => Promise<QueryPageResponse>;
  busy: boolean;
  readOnly: boolean;
  navigationEpoch: number;
  navigationSeq: () => number;
  canRead: () => boolean;
  /** Focuses the title, the next step of an empty folder without an available "Up". */
  focusTitle: () => void;
  /** Focuses "Up" when it is available; false when it is not. */
  focusUp: () => boolean;
  /** Receives this list's entry focus, which the panel calls after opening it. */
  listEntry: RefObject<ListEntryFocus | undefined>;
  /** Receives this list's row focus by index; see {@link ListRowFocus}. */
  listRow: RefObject<ListRowFocus | undefined>;
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
  // The column shows once a row of this list carries the boundary fact; a
  // producer without it gets no column (F02 README #27).
  const scanStatusColumn = pages.boundaries;
  const columns = [
    text("ui.browse.column_name", "Name"),
    text("ui.browse.column_type", "Type"),
    text("ui.browse.column_logical_size", "Logical size"),
    text("ui.browse.column_allocated_size", "Allocated size"),
    text("ui.browse.column_modified", "Modified"),
    ...(scanStatusColumn
      ? [text("ui.browse.column_scan_status", "Scan status")]
      : []),
  ];
  const scrollRoot = useRef<HTMLDivElement>(null);
  // A row is named by its name and its badge only; the controls beside them
  // keep their own names.
  const labels = useId();
  const nameId = (row: FolderQueryRow) => `${labels}name-${rowKey(row.item)}`;
  const badgeId = (row: FolderQueryRow) => `${labels}badge-${rowKey(row.item)}`;
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
    focusEmpty,
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
  const notice = browseNotice(
    incomplete,
    pages.unexplainedLowerBounds,
    dedupCapacityLowerBound,
    text,
  );
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
  /** An empty folder's next step is "Up"; without it, the title. */
  function focusEmpty() {
    if (!focusUp()) focusTitle();
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
   * empty. The first row is focused once the grid has drawn and positioned
   * it, while focus still rests on the control that opened the folder or was
   * lost with it. A row that is in the document but not yet positioned does
   * not take focus, so the grid's own request decides when it can.
   */
  function focusEntry(origin: Element) {
    if (pages.count === 0) {
      focusEmpty();
      return;
    }
    requestRow(0, () => {
      const active = document.activeElement;
      return active === null || active === document.body || active === origin;
    });
  }
  useLayoutEffect(() => {
    listEntry.current = focusEntry;
    listRow.current = (index) => {
      if (pages.count === 0) {
        focusEmpty();
        return;
      }
      requestRow(Math.min(index, pages.count - 1), () => {
        const active = document.activeElement;
        return active === null || active === document.body;
      });
    };
  });
  return (
    <div className="du-browse-results">
      <ResultsNotice lines={notice} />
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
        <PriorityToast priority={priority} />
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
        rowLabelledBy={(row) =>
          priorityBadge(row, priority)
            ? `${nameId(row)} ${badgeId(row)}`
            : nameId(row)
        }
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
                      label: text("ui.browse.inspect", "View details"),
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
            <div className="du-browse-name">
              <div id={nameId(row)}>
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
              </div>
              <PrioritySlot
                row={row}
                priority={priority}
                readOnly={readOnly}
                badgeId={badgeId(row)}
              />
            </div>
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
                        "Link (not followed)",
                      )
                    : // A kind this screen has no name for; the kind was decided.
                      text("ui.common.unknown", "Unknown")}
          </Text>,
          ...[row.logicalSize, row.allocatedSize].map((value) => (
            <Text kind={notCounted(row, value) ? "muted" : "body"}>
              <Unbroken>{size(row, value, display, text)}</Unbroken>
            </Text>
          )),
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
          ...(scanStatusColumn ? [<Text>{scanStatus(row, text)}</Text>] : []),
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
