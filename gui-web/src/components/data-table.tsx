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
}: {
  label: string;
  columns: string[];
  rows: readonly Row[];
  renderCells: (row: Row) => readonly ReactNode[];
  columnWidths?: number[];
}) {
  const [scrollTop, setScrollTop] = useState(0);
  const widths = columns.map((_, index) => columnWidths?.[index] ?? 160);
  const first = Math.min(
    Math.floor(scrollTop / ROW_HEIGHT),
    Math.max(0, rows.length - VIEW_ROWS),
  );
  const start = Math.max(0, first - OVERSCAN);
  const end = Math.min(rows.length, first + VIEW_ROWS + OVERSCAN);
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
        aria-rowcount={rows.length + 1}
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
                style={{ height: start * ROW_HEIGHT }}
              />
            </tr>
          )}
          {rows.slice(start, end).map((row, index) => (
            <tr key={start + index} aria-rowindex={start + index + 2}>
              {renderCells(row).map((cell, column) => (
                <td key={column}>
                  <div
                    className="du-table-cell"
                    style={{ blockSize: ROW_HEIGHT }}
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
                style={{ height: (rows.length - end) * ROW_HEIGHT }}
              />
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
