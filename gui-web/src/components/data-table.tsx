import { useState } from "react";
import type { ReactNode, UIEvent } from "react";
const ROW_HEIGHT = 144;
const VIEW_ROWS = 4;
const OVERSCAN = 3;

/** A fixed-height, keyboard-scrollable table that renders only nearby rows. */
export function DataTable<Row>({
  label,
  columns,
  rows,
  renderCells,
  columnWidths,
  density = "comfortable",
  totalRows,
  getRowKey,
  getRowLabel,
}: {
  label: string;
  columns: string[];
  rows: readonly Row[];
  renderCells: (row: Row, index: number) => readonly ReactNode[];
  columnWidths?: number[];
  density?: "comfortable" | "compact";
  /** Total known rows, or -1 when only a lower bound is known. */
  totalRows?: number;
  getRowKey?: (row: Row) => string;
  /** Accessible row name, for rows whose cells alone do not identify them. */
  getRowLabel?: (row: Row, index: number) => string;
}) {
  const [scrollTop, setScrollTop] = useState(0);
  const rowHeight = density === "compact" ? 64 : ROW_HEIGHT;
  const viewRows = density === "compact" ? 10 : VIEW_ROWS;
  const overscan = density === "compact" ? 10 : OVERSCAN;
  const widths = columns.map((_, index) => columnWidths?.[index] ?? 160);
  const first = Math.min(
    Math.floor(scrollTop / rowHeight),
    Math.max(0, rows.length - viewRows),
  );
  const start = Math.max(0, first - overscan);
  const end = Math.min(rows.length, first + viewRows + overscan);
  const onScroll = (event: UIEvent<HTMLDivElement>) =>
    setScrollTop(event.currentTarget.scrollTop);
  return (
    <div
      className="du-table-viewport"
      role="region"
      aria-label={`${label} scroll area`}
      tabIndex={0}
      onScroll={onScroll}
    >
      <table
        className="du-data-table"
        aria-label={label}
        aria-rowcount={totalRows === -1 ? -1 : (totalRows ?? rows.length) + 1}
        style={{
          minInlineSize:
            rows.length === 0
              ? 0
              : widths.reduce((sum, width) => sum + width, 0),
        }}
      >
        <colgroup>
          {widths.map((width, index) => (
            <col key={index} style={{ width }} />
          ))}
        </colgroup>
        <thead>
          <tr aria-rowindex={1}>
            {columns.map((column) => (
              <th scope="col" key={column}>
                <div>{column}</div>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {start > 0 && (
            <tr aria-hidden="true" className="du-table-spacer">
              <td
                colSpan={columns.length}
                style={{ height: start * rowHeight }}
              />
            </tr>
          )}
          {rows.slice(start, end).map((row, index) => (
            <tr
              key={getRowKey?.(row) ?? start + index}
              aria-rowindex={start + index + 2}
              aria-label={getRowLabel?.(row, start + index)}
            >
              {renderCells(row, start + index).map((cell, column) => (
                <td key={column}>
                  <div
                    className="du-table-cell"
                    style={{ blockSize: rowHeight }}
                  >
                    {cell}
                  </div>
                </td>
              ))}
            </tr>
          ))}
          {end < rows.length && (
            <tr aria-hidden="true" className="du-table-spacer">
              <td
                colSpan={columns.length}
                style={{ height: (rows.length - end) * rowHeight }}
              />
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
