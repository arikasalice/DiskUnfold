import { useId, useState } from "react";
import type { FocusEvent, ReactNode } from "react";
import { useDocumentRows } from "./document-rows";
import "../theme/browse-table.css";

export function BrowseTable<Row>({
  label,
  columns,
  rows,
  totalRows,
  getRowKey,
  renderCells,
}: {
  label: string;
  columns: readonly string[];
  rows: readonly Row[];
  totalRows: number;
  getRowKey: (row: Row) => string;
  renderCells: (row: Row) => readonly ReactNode[];
}) {
  const id = useId();
  const { frame, body, watchRow, layout } = useDocumentRows(rows.length);
  const [focusedRow, setFocusedRow] = useState<{
    index: number;
    key: string;
  }>();
  const indexes = Array.from(
    { length: layout.end - layout.start },
    (_, index) => layout.start + index,
  );
  const pinnedRow = focusedRow && rows[focusedRow.index];
  if (
    focusedRow &&
    pinnedRow !== undefined &&
    focusedRow.index < rows.length &&
    getRowKey(pinnedRow) === focusedRow.key
  ) {
    if (focusedRow.index < layout.start) indexes.unshift(focusedRow.index);
    if (focusedRow.index >= layout.end) indexes.push(focusedRow.index);
  }

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
