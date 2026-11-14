import type { ReactNode } from "react";
import { Toolbar } from "react-aria-components";
import "../theme/selection-actions.css";

/**
 * The action bar for the current selection of a list.
 *
 * `summary` describes the selection. It is not a live region; assistive
 * technology already announces selection changes on the list itself. The
 * toolbar keeps one tab stop and moves between its actions with arrow keys.
 */
export function SelectionActions({
  label,
  summary,
  children,
}: {
  label: string;
  summary: string;
  children: ReactNode;
}) {
  return (
    <Toolbar aria-label={label} className="du-selection-actions">
      <span className="du-selection-actions__summary">{summary}</span>
      <div className="du-selection-actions__items">{children}</div>
    </Toolbar>
  );
}
