import type { ReactNode, Ref } from "react";
import { Toolbar } from "react-aria-components";
import "../theme/text-fit.css";
import "../theme/selection-actions.css";

/**
 * An outcome to announce; a new `id` announces it again. `text` may draw names
 * with `NameText`, so the status region reads the real name and its
 * explanation of edge white space.
 */
export type SelectionNotice = {
  id: number;
  text: ReactNode;
};

/**
 * The action bar for the current selection of a list.
 *
 * `summary` describes the selection. It is not a live region; assistive
 * technology already announces selection changes on the list itself.
 * `summaryId` names the summary so an unavailable action can refer to it.
 *
 * `notice` is announced once by a status region that stays mounted and is
 * not drawn; the owner shows its words on screen in the shared `Toast`
 * (F06 README 35), and the summary stays. This bar sets no timer of its own.
 * The toolbar keeps one tab stop and moves between its actions with arrow keys.
 */
export function SelectionActions({
  label,
  summary,
  summaryId,
  notice,
  ref,
  children,
}: {
  label: string;
  summary: string;
  summaryId?: string;
  notice?: SelectionNotice | undefined;
  ref?: Ref<HTMLDivElement>;
  children: ReactNode;
}) {
  return (
    <Toolbar ref={ref} aria-label={label} className="du-selection-actions">
      <div className="du-selection-actions__message">
        <span className="du-selection-actions__summary" id={summaryId}>
          {summary}
        </span>
        <span className="du-visually-hidden" role="status">
          {notice && <span key={notice.id}>{notice.text}</span>}
        </span>
      </div>
      <div className="du-selection-actions__items">{children}</div>
    </Toolbar>
  );
}
