import type { ReactNode, Ref } from "react";
import { Toolbar } from "react-aria-components";
import { Icon } from "./presentation";
import type { IconName } from "./presentation";
import "../theme/selection-actions.css";

/**
 * An outcome shown in place of the summary; a new `id` announces it again.
 * `text` may draw names with `NameText`, so the status region reads the real
 * name and its explanation while the marks stay on screen only.
 */
export type SelectionNotice = {
  id: number;
  tone: "success" | "info" | "error";
  text: ReactNode;
};

const noticeIcons = {
  success: "success",
  info: "info",
  error: "error",
} satisfies Record<SelectionNotice["tone"], IconName>;

/**
 * The action bar for the current selection of a list.
 *
 * `summary` describes the selection. It is not a live region; assistive
 * technology already announces selection changes on the list itself.
 * `summaryId` names the summary so an unavailable action can refer to it.
 *
 * `notice` replaces the summary while its owner keeps it; this bar sets no
 * timer of its own. A status region that stays mounted announces each new
 * notice once, and its icon shows the outcome by shape as well as colour. The
 * toolbar keeps one tab stop and moves between its actions with arrow keys.
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
        <span
          className="du-selection-actions__summary"
          id={summaryId}
          hidden={notice !== undefined}
        >
          {summary}
        </span>
        <span className="du-selection-actions__status" role="status">
          {notice && (
            <span
              key={notice.id}
              className="du-selection-actions__notice"
              data-tone={notice.tone}
            >
              <Icon name={noticeIcons[notice.tone]} />
              <span>{notice.text}</span>
            </span>
          )}
        </span>
      </div>
      <div className="du-selection-actions__items">{children}</div>
    </Toolbar>
  );
}
