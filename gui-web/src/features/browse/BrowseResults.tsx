import { useLayoutEffect, useRef } from "react";
import {
  Action,
  BrowseGrid,
  ItemActions,
  ActionFeedback,
  Notice,
  Text,
} from "../../components";
import type {
  FolderQueryRow,
  BrowseCopyPathOutcome,
  ItemRef,
  QueryPageRequest,
  QueryPageResponse,
} from "../../ipc";
import type { BrowsePages } from "../../state/browse-pages";
import { browseDisplayName, rowKey } from "./browse-display";
import { formatByteUnit } from "../../i18n/bytes";
import { SIZE_UNIT_RULES } from "../../ipc";
import { useBrowsePaging } from "./use-browse-paging";
import { useBrowseCopy } from "./use-browse-copy";

const columns = [
  "Name",
  "Type",
  "Logical size",
  "Allocated size",
  "Modified",
  "Availability",
];

function size(value: FolderQueryRow["logicalSize"]): string {
  if (value.state === "absent") return "Not recorded";
  if (value.state !== "present" || value.value.state === "unknown")
    return "Unavailable";
  const lowerBound = value.value.state === "lower_bound";
  const readable = formatByteUnit(
    BigInt(value.value.bytes),
    SIZE_UNIT_RULES.iec,
    SIZE_UNIT_RULES.decimalPlaces,
    "auto",
    lowerBound,
  );
  return `${lowerBound ? "At least " : ""}${readable}`;
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

export function BrowseResults({
  pages,
  initialCursor,
  readPage,
  busy,
  readOnly,
  navigationEpoch,
  navigationSeq,
  canRead,
  focusPanel,
  openFolder,
  openItem,
  copyPath,
  reopen,
}: {
  pages: BrowsePages;
  initialCursor: string | null;
  readPage: (request: QueryPageRequest) => Promise<QueryPageResponse>;
  busy: boolean;
  readOnly: boolean;
  navigationEpoch: number;
  navigationSeq: () => number;
  canRead: () => boolean;
  focusPanel: () => void;
  openFolder: (item: ItemRef) => void;
  openItem:
    ((row: FolderQueryRow, trigger: Element | null) => void) | undefined;
  copyPath: ((item: ItemRef) => Promise<BrowseCopyPathOutcome>) | undefined;
  reopen: () => void;
}) {
  const scrollRoot = useRef<HTMLDivElement>(null);
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
  return (
    <div className="du-browse-results">
      <div className="du-browse-controls">
        {pages.count === 0 && <Text>No observed items in this folder.</Text>}
        {paging.failure && (
          <Notice tone="error">
            {paging.failure.stale
              ? "This result can no longer be read. Reopen this folder to continue."
              : "Could not load the requested page."}{" "}
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
                  ? "Reopening folder…"
                  : "Reopen folder"
                : paging.retrying
                  ? "Retrying page…"
                  : "Retry page"}
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
                {paging.loadingMore ? "Loading…" : "Load more"}
              </Action>
            )}
          </div>
        )}
      </div>
      <BrowseGrid<FolderQueryRow>
        scrollRoot={scrollRoot}
        label="Folder items"
        columns={columns}
        count={pages.count}
        rowCount={pages.rowCount}
        busy={paging.pending}
        focusRequest={paging.focusRequest}
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
        renderCells={(row) => [
          <ItemActions
            rowScope
            label={`Actions for ${browseDisplayName(row.displayName)}`}
            actions={[
              ...((row.kind === "file" || row.kind === "directory") && openItem
                ? [
                    {
                      id: "inspect",
                      label: "Inspect",
                      onAction: (trigger: HTMLElement | null) =>
                        openItem(row, trigger),
                      ...(busy
                        ? { disabledReason: "Unavailable while opening" }
                        : {}),
                    },
                  ]
                : []),
              {
                id: "copy",
                label: "Copy path",
                onAction: (trigger: HTMLElement | null) =>
                  copy.copy(row, trigger),
                ...(!copyPath || busy || copy.busy
                  ? { disabledReason: "Unavailable now" }
                  : {}),
              },
            ]}
          >
            {row.kind === "directory" ? (
              <Action
                excludeFromTabOrder
                isDisabled={readOnly}
                onPress={() => openFolder(row.item)}
              >
                {browseDisplayName(row.displayName)}
              </Action>
            ) : row.kind === "file" && openItem ? (
              <Action
                excludeFromTabOrder
                isDisabled={busy}
                onPress={(event) => openItem(row, event.target)}
              >
                {browseDisplayName(row.displayName)}
              </Action>
            ) : (
              <Text>{browseDisplayName(row.displayName)}</Text>
            )}
          </ItemActions>,
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
      <ActionFeedback
        status={
          copy.status ||
          (paging.moving ? "Loading folder items…" : paging.status) ||
          ""
        }
        parts={copy.feedback}
        pending={copy.busy || !copy.status}
        onDismiss={copy.dismiss}
      />
    </div>
  );
}
