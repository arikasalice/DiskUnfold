import type { ReactNode } from "react";
import { useLocaleText } from "../i18n/presentation";
import "../theme/sorted-table.css";

export type SortDirection = "ascending" | "descending";
export type TableSort = { column: string; direction: SortDirection };
export type SortedColumn = {
  id: string;
  label: string;
  /** Without it the header shows no sort mark and cannot be pressed. */
  sortable?: boolean;
  /** The direction a first press chooses; ascending by default. */
  firstDirection?: SortDirection;
};

/**
 * A table whose sortable column headers are buttons and whose header row stays
 * on top while its region scrolls. The first cell of a row names the row.
 */
export function SortedTable<Row>({
  label,
  columns,
  rows,
  getKey,
  renderCells,
  sort,
  onSort,
  footer,
}: {
  label: string;
  columns: readonly SortedColumn[];
  rows: readonly Row[];
  getKey: (row: Row) => string;
  renderCells: (row: Row) => readonly ReactNode[];
  sort: TableSort;
  onSort: (sort: TableSort) => void;
  /** Below the rows inside the scrolling region, such as "Load more". */
  footer?: ReactNode;
}) {
  const text = useLocaleText();
  return (
    <div
      className="du-sorted-table"
      role="region"
      aria-label={text("ui.common.scroll_area", "{name} scroll area", {
        name: label,
      })}
      tabIndex={0}
    >
      <table aria-label={label}>
        <thead>
          <tr>
            {columns.map((column) => {
              const current = sort.column === column.id;
              return (
                <th
                  key={column.id}
                  scope="col"
                  aria-sort={
                    current
                      ? sort.direction
                      : column.sortable
                        ? "none"
                        : undefined
                  }
                >
                  {column.sortable ? (
                    <button
                      type="button"
                      className="du-sorted-table__sort"
                      onClick={() =>
                        onSort({
                          column: column.id,
                          direction: current
                            ? sort.direction === "ascending"
                              ? "descending"
                              : "ascending"
                            : (column.firstDirection ?? "ascending"),
                        })
                      }
                    >
                      {column.label}
                      <span
                        aria-hidden="true"
                        className="du-sorted-table__mark"
                      >
                        {current
                          ? sort.direction === "ascending"
                            ? "▲"
                            : "▼"
                          : ""}
                      </span>
                    </button>
                  ) : (
                    column.label
                  )}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={getKey(row)}>
              {renderCells(row).map((cell, index) =>
                index === 0 ? (
                  <th key={index} scope="row">
                    {cell}
                  </th>
                ) : (
                  <td key={index}>{cell}</td>
                ),
              )}
            </tr>
          ))}
        </tbody>
      </table>
      {footer}
    </div>
  );
}
