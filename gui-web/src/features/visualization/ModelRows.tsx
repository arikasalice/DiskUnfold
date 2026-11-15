import { useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";
import { Action, DataTable, DetailDisclosure, Text } from "../../components";
import type { VizModel, VizRow, VizRowsRequest } from "../../ipc";
import {
  RowName,
  plainRowName,
  rowName,
  rowRatioText,
  vizMessage,
} from "../../viz";
import type { ModelRowsPage, VizText } from "../../viz";

/** Class keys that mean a row has no classification. */
const UNCLASSIFIED = new Set([
  "unclassified",
  "ui.visualization.legend.unclassified",
]);
/** A row of the table: one fixed height, so the table is as tall as its rows. */
const ROW_HEIGHT = 48;
/** Notes the value column already shows (unconfirmed, lower bound); they are not written again. */
const SHOWN_IN_VALUE = new Set(["viz.flag.unknown", "viz.flag.lower_bound"]);
const notesOf = (flags: readonly string[]) =>
  flags.filter((flag) => !SHOWN_IN_VALUE.has(flag));

type Column = { header: string; cell: (row: VizRow) => ReactNode };

function denominatorWords(row: VizRow, text: VizText) {
  return `${text.key(row.denominator.basis)} ${text.quantity(row.denominator.value)}`;
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
}) {
  const rowTitle = title ?? text.message("rowsFromGraph");
  const rowsRegion = useRef<HTMLElement>(null);
  const { request, response } = page;
  const { rows } = response;
  const next = request.start + rows.length;
  const paged = request.start > 0 || next < response.totalCount;
  const hasOpenableGroups =
    onOpenGroup !== undefined &&
    rows.some((row) => row.groupRef !== null && row.itemRef === null);
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
      cell: (row) => <RowName name={nameOf(row)} item={row.itemRef !== null} />,
    },
    ...(hasOpenableGroups
      ? [
          {
            header: text.message("actions"),
            cell: (row: VizRow) =>
              row.groupRef !== null && row.itemRef === null && onOpenGroup ? (
                <OpenGroup
                  row={row}
                  name={nameOf(row)}
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
      cell: (row) =>
        row.quantity ? text.quantity(row.quantity) : text.size(row.size),
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
            cell: (row: VizRow) =>
              text.key(
                model?.legend.find((entry) => entry.key === row.legendClassKey)
                  ?.labelKey ?? row.legendClassKey,
              ),
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
        defaultExpanded={expanded}
        key={`${request.modelRef}:${request.resultRevision}`}
      >
        {sharedDenominator !== null && (
          <Text>
            {vizMessage(text, "ratioDenominator", { value: sharedDenominator })}
          </Text>
        )}
        <Text>{pageWords(page, paged, text)}</Text>
        <DataTable
          label={text.message("loadedGraphRows")}
          density="compact"
          flat
          rowHeight={ROW_HEIGHT}
          columns={columns.map(({ header }) => header)}
          rows={rows}
          getRowKey={(row) => JSON.stringify([row.itemRef, row.groupRef])}
          renderCells={(row) => columns.map(({ cell }) => cell(row))}
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

/** The line above the table: how many items, or which of them this page shows. */
function pageWords(page: ModelRowsPage, paged: boolean, text: VizText) {
  const { request, response } = page;
  if (response.rows.length === 0) return text.message("noRowsInPage");
  if (!paged)
    return vizMessage(text, "rowsOnePage", {
      count: String(response.totalCount),
    });
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
  text,
  onOpenGroup,
  returnTarget,
}: {
  row: VizRow;
  name: string;
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
      {text.message("viewMembers")}
    </Action>
  );
}
