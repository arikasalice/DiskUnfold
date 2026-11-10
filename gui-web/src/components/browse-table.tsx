import { useId, useLayoutEffect, useState } from "react";
import type { FocusEvent, ReactNode, RefObject } from "react";
import { useDocumentRows } from "./document-rows";
import "../theme/browse-table.css";

export function BrowseTable<Row>({
  label,
  columns,
  rows,
  pageFocus,
  scrollFooter,
  totalRows,
  getRowKey,
  renderCells,
}: {
  label: string;
  columns: readonly string[];
  rows: readonly Row[];
  pageFocus?:
    | {
        index: number;
        scrollY: number;
        button: RefObject<HTMLButtonElement | null>;
      }
    | undefined;
  scrollFooter?: RefObject<HTMLButtonElement | null>;
  totalRows: number;
  getRowKey: (row: Row) => string;
  renderCells: (row: Row) => readonly ReactNode[];
}) {
  const id = useId();
  const { frame, body, watchRow, layout } = useDocumentRows(
    rows.length,
    scrollFooter,
  );
  const [focusedRow, setFocusedRow] = useState<{
    index: number;
    key: string;
  }>();
  const [completedFocus, setCompletedFocus] = useState<number>();
  const pendingFocus =
    pageFocus?.index === completedFocus ? undefined : pageFocus;
  const pendingIndex = pendingFocus?.index;
  const indexes = Array.from(
    { length: layout.end - layout.start },
    (_, index) => layout.start + index,
  );
  const pinnedRow = focusedRow && rows[focusedRow.index];
  const pin =
    pendingIndex ??
    (focusedRow &&
    pinnedRow !== undefined &&
    getRowKey(pinnedRow) === focusedRow.key
      ? focusedRow.index
      : undefined);
  if (pin !== undefined && pin < rows.length) {
    if (pin < layout.start) indexes.unshift(pin);
    if (pin >= layout.end) indexes.push(pin);
  }

  useLayoutEffect(() => {
    if (pendingFocus === undefined) return;
    setCompletedFocus(pendingFocus.index);
    // The button remains for another page, or was removed by a terminal response.
    const expected = pendingFocus.button.current ?? document.body;
    if (
      document.activeElement !== expected ||
      window.scrollY !== pendingFocus.scrollY
    )
      return;
    const row = body.current?.querySelector<HTMLElement>(
      `[data-row="${pendingFocus.index}"]`,
    );
    const action = row?.querySelector<HTMLButtonElement>(
      "button:not(:disabled)",
    );
    (action ?? row)?.focus();
  }, [body, pendingFocus]);

  function trackFocus(event: FocusEvent<HTMLTableSectionElement>) {
    const index = Number(
      event.target.closest<HTMLElement>("[data-row]")?.dataset.row,
    );
    const row = rows[index];
    setFocusedRow(
      row === undefined ? undefined : { index, key: getRowKey(row) },
    );
  }

  function renderGap(from: number, to: number) {
    if (from >= to) return null;
    const height = (layout.offsets[to] ?? 0) - (layout.offsets[from] ?? 0);
    return (
      <tr
        aria-hidden="true"
        className="du-browse-table__gap"
        key={`gap-${from}`}
      >
        <td colSpan={columns.length} style={{ blockSize: height }} />
      </tr>
    );
  }

  return (
    <div ref={frame} className="du-browse-table-frame">
      <table
        role="table"
        className="du-browse-table"
        aria-label={label}
        aria-rowcount={totalRows === -1 ? -1 : totalRows + 1}
      >
        <thead role="rowgroup">
          <tr role="row" aria-rowindex={1}>
            {columns.map((column, index) => (
              <th
                role="columnheader"
                id={`${id}-${index}`}
                scope="col"
                key={column}
              >
                {column}
              </th>
            ))}
          </tr>
        </thead>
        <tbody
          ref={body}
          role="rowgroup"
          onFocus={trackFocus}
          onBlur={(event) => {
            if (
              !(event.relatedTarget instanceof Node) ||
              !event.currentTarget.contains(event.relatedTarget)
            ) {
              setFocusedRow(undefined);
            }
          }}
        >
          {indexes.flatMap((index, position) => {
            const row = rows[index];
            if (row === undefined) return [];
            return [
              renderGap((indexes[position - 1] ?? -1) + 1, index),
              <tr
                ref={watchRow}
                role="row"
                key={getRowKey(row)}
                data-row={index}
                tabIndex={index === pin ? -1 : undefined}
                aria-rowindex={index + 2}
              >
                {renderCells(row).map((cell, column) => (
                  <td role="cell" headers={`${id}-${column}`} key={column}>
                    <span className="du-browse-table__label" aria-hidden="true">
                      {columns[column]}
                    </span>
                    {cell}
                  </td>
                ))}
              </tr>,
            ];
          })}
          {renderGap((indexes.at(-1) ?? -1) + 1, rows.length)}
        </tbody>
      </table>
    </div>
  );
}
