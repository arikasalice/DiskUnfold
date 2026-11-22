import { useContext, useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";
import {
  Action,
  ChartFallbackContext,
  DataTable,
  DetailDisclosure,
  StackedBar,
  Text,
} from "../../components";
import type { VizModel, VizRow, VizRowsRequest } from "../../ipc";
import {
  RowName,
  classCell,
  legendClassName,
  plainRowName,
  rowName,
  rowRatioText,
  rowFolderTypes,
  sankeyLinkTarget,
  vizMessage,
} from "../../viz";
import type { ModelRowsPage, VizText } from "../../viz";
import type { RowReveal } from "./UndrawnLine";

/** Class keys that mean a row has no classification. */
const UNCLASSIFIED = new Set([
  "unclassified",
  "ui.visualization.legend.unclassified",
]);
/** A row of the table: one fixed height, so the table is as tall as its rows. */
const ROW_HEIGHT = 48;
/** The length of a folder's type bar in the class cell (README 54.6). */
const TYPE_BAR_WIDTH = 96;
/** A page with a folder's unread line under its value gives every row room for it. */
const TWO_LINE_ROW_HEIGHT = 64;
/** Notes the value column already shows (unconfirmed, lower bound); they are not written again. */
const SHOWN_IN_VALUE = new Set(["viz.flag.unknown", "viz.flag.lower_bound"]);
const notesOf = (flags: readonly string[]) =>
  flags.filter((flag) => !SHOWN_IN_VALUE.has(flag));

type Column = { header: string; cell: (row: VizRow) => ReactNode };

/**
 * "Current scope 1.1 MiB", or "Current scope · Unavailable" when the value is
 * unknown, so it does not read as an unavailable scope (design README 28).
 */
function denominatorWords(row: VizRow, text: VizText) {
  const separator = row.denominator.value.state === "unknown" ? " · " : " ";
  return `${text.key(row.denominator.basis)}${separator}${text.quantity(row.denominator.value)}`;
}

/**
 * The rows of one model as a table. The classification column waits for a
 * colour criterion, the notes column appears only when the graph has a note to
 * make, and a denominator every row shares is said once above the table.
 */
export function ModelRows({
  page,
  model,
  text,
  onPage,
  onOpenGroup,
  returnTargetRef,
  title,
  expanded = false,
  reveal,
}: {
  page: ModelRowsPage;
  /** Absent for a fallback table, which has no model of its own to look classes up in. */
  model?: VizModel | undefined;
  text: VizText;
  onPage?: (request: VizRowsRequest) => void;
  onOpenGroup?: (
    groupRef: string,
    size: VizRow["size"],
    origin: HTMLElement,
    groupLabel: string,
    returnTarget: HTMLElement | null,
  ) => void;
  returnTargetRef?: { current: HTMLElement | null } | undefined;
  title?: string;
  /** Opens the section when it first shows; the rows are what a reader came for. */
  expanded?: boolean;
  /** A row to open the section on and focus (F06 README 51.3, 48.4). */
  reveal?: RowReveal | null;
}) {
  const rowTitle = title ?? defaultTitle(model, text);
  // In the chart's place the rows open expanded; under a chart they wait
  // folded, since each shape has its info card (README 22, 24).
  const open = useContext(ChartFallbackContext) || expanded;
  const rowsRegion = useRef<HTMLElement>(null);
  const { request, response } = page;
  const { rows } = response;
  const next = request.start + rows.length;
  const revealed = revealInPage(reveal, request.start, next, open);
  const paged = request.start > 0 || next < response.totalCount;
  const hasOpenableGroups = onOpenGroup !== undefined && rows.some(openable);
  const showClassification = rows.some(
    (row) => !UNCLASSIFIED.has(row.legendClassKey),
  );
  const showNotes =
    rows.some((row) => notesOf(row.displayFlags).length > 0) ||
    (model?.shapes.some((shape) => notesOf(shape.displayFlags).length > 0) ??
      false);
  const sharedDenominator =
    rows.length > 0 &&
    rows.every(
      (row) => denominatorWords(row, text) === denominatorWords(rows[0]!, text),
    )
      ? denominatorWords(rows[0]!, text)
      : null;
  useLayoutEffect(() => {
    if (returnTargetRef) returnTargetRef.current = rowsRegion.current;
  }, [
    request.modelRef,
    request.resultRevision,
    request.start,
    response.rows,
    returnTargetRef,
  ]);
  const nameOf = (row: VizRow) =>
    // A fallback table has no model, so a group row keeps only a name it carries itself.
    model
      ? rowName(row, model, text)
      : (plainRowName(row) ?? text.message("group.other_unknown"));
  const columns: Column[] = [
    {
      header: text.message("name"),
      cell: (row) => (
        <RowName
          name={nameOf(row)}
          item={row.itemRef !== null}
          folder={row.itemKind === "folder"}
        />
      ),
    },
    ...(hasOpenableGroups
      ? [
          {
            header: text.message("actions"),
            cell: (row: VizRow) =>
              openable(row) && onOpenGroup ? (
                <OpenGroup
                  row={row}
                  name={nameOf(row)}
                  label={openLabel(model, text)}
                  text={text}
                  onOpenGroup={onOpenGroup}
                  returnTarget={() =>
                    returnTargetRef?.current ?? rowsRegion.current
                  }
                />
              ) : null,
          },
        ]
      : []),
    {
      header: text.message("value"),
      cell: (row) => rowValue(row, text),
    },
    {
      header: text.message("ratio"),
      cell: (row) => rowRatioText(row, text),
    },
    ...(sharedDenominator === null
      ? [
          {
            header: text.message("denominator"),
            cell: (row: VizRow) => denominatorWords(row, text),
          },
        ]
      : []),
    ...(showClassification
      ? [
          {
            header: text.message("classification"),
            cell: (row: VizRow) => classification(model, row, text),
          },
        ]
      : []),
    ...(showNotes
      ? [
          {
            header: text.message("rowNotes"),
            cell: (row: VizRow) =>
              notesOf(row.displayFlags).map(text.key).join(" · "),
          },
        ]
      : []),
    ...linkColumns(rows, model, text),
  ];
  return (
    <div
      ref={(element) => {
        rowsRegion.current = element;
        if (returnTargetRef) returnTargetRef.current = element;
      }}
      {...(returnTargetRef
        ? { role: "group", tabIndex: -1, "aria-label": rowTitle }
        : {})}
    >
      <DetailDisclosure
        title={rowTitle}
        defaultExpanded={revealed.expand}
        // A reveal opens the section again even after the reader folded it.
        key={`${request.modelRef}:${request.resultRevision}:${revealed.key}`}
      >
        {sharedDenominator !== null && (
          <Text>
            {vizMessage(text, "ratioDenominator", { value: sharedDenominator })}
          </Text>
        )}
        <Text>{pageWords(page, paged, text, model)}</Text>
        <DataTable
          label={rowTitle}
          density="compact"
          flat
          rowHeight={rowHeight(rows)}
          // The action column holds only buttons; its header would read as a value.
          hiddenHeaders={[text.message("actions")]}
          columns={columns.map(({ header }) => header)}
          rows={rows}
          getRowKey={(row) => JSON.stringify([row.itemRef, row.groupRef])}
          renderCells={(row) => columns.map(({ cell }) => cell(row))}
          focusRow={revealed.focusRow}
        />
        {onPage && paged && (
          <div className="du-review-actions">
            <Action
              isDisabled={request.start === 0}
              onPress={() =>
                onPage({
                  ...request,
                  start: Math.max(0, request.start - request.count),
                })
              }
            >
              {text.message("previousRows")}
            </Action>
            <Action
              isDisabled={next >= response.totalCount || rows.length === 0}
              onPress={() => onPage({ ...request, start: next })}
            >
              {text.message("nextRows")}
            </Action>
          </div>
        )}
      </DetailDisclosure>
    </div>
  );
}

/**
 * The reveal of a row on this page: the section opens and the row takes the
 * focus. Without one the section opens as it otherwise would (open).
 */
function revealInPage(
  reveal: RowReveal | null | undefined,
  start: number,
  end: number,
  open: boolean,
) {
  if (!reveal || reveal.row < start || reveal.row >= end)
    return { expand: open, key: "", focusRow: null };
  return {
    expand: true,
    key: String(reveal.token),
    focusRow: { index: reveal.row - start, token: reveal.token },
  };
}

/**
 * The classification cell. A row the type does not apply to reads "—", named
 * "No category" for assistive technology (F06 README 51, TERMS 「유형」).
 */
function classification(
  model: VizModel | undefined,
  row: VizRow,
  text: VizText,
) {
  // A folder by its types: the bar and its largest type (README 54.6).
  const types = rowFolderTypes(model, row, text);
  if (types && types.bar.length > 0)
    return (
      <>
        <StackedBar
          slices={types.bar}
          width={TYPE_BAR_WIDTH}
          {...(types.total === undefined ? {} : { total: types.total })}
        />
        <div>{types.cell}</div>
      </>
    );
  if (!types && !UNCLASSIFIED.has(row.legendClassKey))
    return classCell(model, row.legendClassKey, text);
  return (
    <>
      <div aria-hidden="true">—</div>
      <div className="du-visually-hidden">{text.message("noCategory")}</div>
    </>
  );
}

/**
 * A row's value. A lower bound of nothing, such as a folder that could not be read
 * at all, is unavailable rather than "at least 0 bytes" (F06 README 48.2, 44.2).
 */
function rowValue(row: VizRow, text: VizText) {
  const value = row.quantity
    ? text.quantity(row.quantity)
    : row.size.state === "lower_bound" && row.size.bytes === "0"
      ? text.message("unavailableValue")
      : text.size(row.size);
  const reason = unreadReason(row, text);
  return reason ? (
    <div>
      <Text>{value}</Text>
      <Text kind="muted">{reason}</Text>
    </div>
  ) : (
    value
  );
}

/** The row height of a page: taller when a folder row says why it is partly known. */
function rowHeight(rows: readonly VizRow[]) {
  return rows.some(
    (row) => row.unread?.kind === "itself" || row.unread?.kind === "inside",
  )
    ? TWO_LINE_ROW_HEIGHT
    : ROW_HEIGHT;
}

/**
 * The "Linked items" column when a Sankey node row sends space on, so the table
 * reaches every link a reader cannot Tab to in the chart (README 12.1).
 */
function linkColumns(
  rows: readonly VizRow[],
  model: VizModel | undefined,
  text: VizText,
): Column[] {
  if (!rows.some((row) => (row.links?.length ?? 0) > 0)) return [];
  return [
    {
      header: text.message("linkedItems"),
      cell: (row) => linkCell(model, row, text),
    },
  ];
}

/** "Video 80 GiB · Documents 3 GiB": where a Sankey node row sends its space. */
function linkCell(model: VizModel | undefined, row: VizRow, text: VizText) {
  const words = {
    ...text,
    className: (key: string) =>
      model ? legendClassName(model, key, text) : text.key(key),
  };
  return (row.links ?? [])
    .map(
      (link) =>
        `${sankeyLinkTarget(model, link.toRowRef, words)} ${text.quantity(link.quantity)}`,
    )
    .join(" · ");
}

/**
 * Why a folder row is only partly known (F06 README 53), in the browse screen's
 * words: its own listing failed, or folders below it could not be read. A kind
 * this screen does not know says nothing.
 */
function unreadReason(row: VizRow, text: VizText) {
  const unread = row.unread;
  if (unread?.kind === "itself") return text.message("unreadItself");
  if (unread?.kind !== "inside") return null;
  return unread.count === undefined
    ? text.message("unreadInsideSome")
    : vizMessage(text, "unreadInside", { count: unread.count });
}

/**
 * A group row whose items can be opened. An empty histogram interval has
 * nothing to open, so it gets no button (README 43).
 */
function openable(row: VizRow) {
  return (
    row.groupRef !== null &&
    row.itemRef === null &&
    !(row.memberCount?.state === "exact" && row.memberCount.value === "0")
  );
}

/** A histogram interval is a size range, not a combined group (README 43). */
function openLabel(model: VizModel | undefined, text: VizText) {
  return text.message(
    model?.representation.format === "histogram"
      ? "viewRangeItems"
      : "viewMembers",
  );
}

/** A histogram lists size ranges, so its table says so (README 43). */
function defaultTitle(model: VizModel | undefined, text: VizText) {
  return text.message(
    model?.representation.format === "histogram"
      ? "rowsBySizeRange"
      : "rowsFromGraph",
  );
}

/** The line above the table: how many items, or which of them this page shows. */
function pageWords(
  page: ModelRowsPage,
  paged: boolean,
  text: VizText,
  model: VizModel | undefined,
) {
  const { request, response } = page;
  if (response.rows.length === 0) return text.message("noRowsInPage");
  if (!paged)
    // A histogram's rows are size ranges; counting them as items would read as the scope's item count.
    return vizMessage(
      text,
      model?.representation.format === "histogram"
        ? "rangesOnePage"
        : "rowsOnePage",
      { count: String(response.totalCount) },
    );
  return vizMessage(text, "rowRange", {
    start: String(request.start + 1),
    end: String(request.start + response.rows.length),
    total: String(response.totalCount),
  });
}

/** The button that opens the combined items of a group row. */
function OpenGroup({
  row,
  name,
  label,
  text,
  onOpenGroup,
  returnTarget,
}: {
  row: VizRow;
  name: string;
  label: string;
  text: VizText;
  onOpenGroup: NonNullable<Parameters<typeof ModelRows>[0]["onOpenGroup"]>;
  returnTarget: () => HTMLElement | null;
}) {
  const trigger = useRef<HTMLButtonElement>(null);
  return (
    <Action
      ref={trigger}
      onPress={() => {
        if (trigger.current && row.groupRef !== null)
          onOpenGroup(
            row.groupRef,
            row.size,
            trigger.current,
            name,
            returnTarget(),
          );
      }}
      aria-label={vizMessage(text, "viewMembersOf", { name })}
    >
      {label}
    </Action>
  );
}
