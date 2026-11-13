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
import type { SizeDisplay } from "../../i18n/bytes";
import { useTimestampText } from "../../i18n/regional-format";
import { useLocaleText } from "../../i18n/presentation";
import { SIZE_UNIT_RULES } from "../../ipc";
import { useBrowsePaging } from "./use-browse-paging";
import { useBrowseCopy } from "./use-browse-copy";

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
    found.push(text("ui.browse.availability_unknown", "Availability unknown"));
  return found.join(" · ") || text("ui.browse.not_reported", "Not reported");
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
  openFolder,
  openItem,
  copyPath,
  reopen,
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
  focusPanel: () => void;
  openFolder: (item: ItemRef) => void;
  openItem:
    ((row: FolderQueryRow, trigger: Element | null) => void) | undefined;
  copyPath: ((item: ItemRef) => Promise<BrowseCopyPathOutcome>) | undefined;
  reopen: () => void;
}) {
  const text = useLocaleText();
  const timestamp = useTimestampText();
  const columns = [
    text("ui.browse.column_name", "Name"),
    text("ui.browse.column_type", "Type"),
    text("ui.browse.column_logical_size", "Logical size"),
    text("ui.browse.column_allocated_size", "Allocated size"),
    text("ui.browse.column_modified", "Modified"),
    text("ui.browse.column_availability", "Availability"),
  ];
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
              {
                id: "copy",
                label: text("ui.browse.copy_path", "Copy path"),
                onAction: (trigger: HTMLElement | null) =>
                  copy.copy(row, trigger),
                ...(!copyPath || busy || copy.busy
                  ? {
                      disabledReason: text(
                        "ui.browse.unavailable_now",
                        "Unavailable now",
                      ),
                    }
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
                {browseDisplayName(row.displayName, text)}
              </Action>
            ) : row.kind === "file" && openItem ? (
              <Action
                excludeFromTabOrder
                isDisabled={busy}
                onPress={(event) => openItem(row, event.target)}
              >
                {browseDisplayName(row.displayName, text)}
              </Action>
            ) : (
              <Text>{browseDisplayName(row.displayName, text)}</Text>
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
          <Text>{size(row.logicalSize, display, text)}</Text>,
          <Text>{size(row.allocatedSize, display, text)}</Text>,
          <Text>
            {row.modifiedAt.state === "present"
              ? timestamp(Date.parse(row.modifiedAt.value), () =>
                  row.modifiedAt.state === "present"
                    ? row.modifiedAt.value
                    : "",
                )
              : row.modifiedAt.state === "absent"
                ? text("ui.browse.not_recorded", "Not recorded")
                : text("ui.browse.unavailable", "Unavailable")}
          </Text>,
          <Text>{availability(row, text)}</Text>,
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
