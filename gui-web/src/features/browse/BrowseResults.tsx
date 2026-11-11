import {
  Action,
  BrowseGrid,
  ItemActions,
  LiveStatus,
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
  openFile,
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
  openFile:
    ((row: FolderQueryRow, trigger: Element | null) => void) | undefined;
  copyPath: ((item: ItemRef) => Promise<BrowseCopyPathOutcome>) | undefined;
  reopen: () => void;
}) {
  const copy = useBrowseCopy(copyPath, navigationEpoch, navigationSeq);
  const paging = useBrowsePaging({
    pages,
    initialCursor,
    readPage,
    enabled: !busy,
    navigationEpoch,
    navigationSeq,
    canRead,
    focusPanel,
  });
  return (
    <>
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
      <div
        className="du-browse-status"
        data-moving={paging.moving || undefined}
      >
        <LiveStatus>
          {paging.moving ? "Loading folder items…" : paging.status}
        </LiveStatus>
      </div>
      <LiveStatus placement="viewport">{copy.status}</LiveStatus>
      {initialCursor !== null && (
        <div className="du-browse-more">
          {paging.showLoadMore && (
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
      <BrowseGrid<FolderQueryRow>
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
          if (row.kind === "file") openFile?.(row, document.activeElement);
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
              ...(row.kind === "file" && openFile
                ? [
                    {
                      id: "inspect",
                      label: "Inspect",
                      onAction: (trigger: HTMLElement | null) =>
                        openFile(row, trigger),
                      ...(busy
                        ? { disabledReason: "Unavailable while opening" }
                        : {}),
                    },
                  ]
                : []),
              {
                id: "copy",
                label: "Copy path",
                onAction: () => copy.copy(row),
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
            ) : row.kind === "file" && openFile ? (
              <Action
                excludeFromTabOrder
                isDisabled={busy}
                onPress={(event) => openFile(row, event.target)}
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
    </>
  );
}
