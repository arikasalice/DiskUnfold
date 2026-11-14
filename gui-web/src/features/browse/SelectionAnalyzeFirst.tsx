import { useId, useLayoutEffect, useRef } from "react";
import { Action, markNameEdges, SelectionActions } from "../../components";
import { scanPriorityText, scanPriorityTone } from "../../ipc";
import type { FolderQueryRow } from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { BrowsePages } from "../../state/browse-pages";
import type { ScanPriority } from "../../state/scan-priority";
import { browseDisplayName, rowKey } from "./browse-display";

/** Every row, or the keys of the selected rows. */
export type Selection = "all" | ReadonlySet<string>;

/**
 * An exact folder size means every item below it was counted, so the folder
 * needs no further collection. A running scan's partial result reports lower
 * bounds, so this does not yet identify folders finished during the scan.
 */
function collected(row: FolderQueryRow): boolean {
  return (
    row.logicalSize.state === "present" &&
    row.logicalSize.value.state === "exact"
  );
}

function excluded(row: FolderQueryRow): boolean {
  const flag = row.flags.excluded;
  return flag?.state === "present" && flag.value;
}

/** "Analyze first" is offered only for uncollected in-scope folders of the scan's current result. */
export function canAnalyzeFirst(
  row: FolderQueryRow,
  priority: ScanPriority | undefined,
  readOnly: boolean,
): priority is ScanPriority {
  return (
    priority !== undefined &&
    !readOnly &&
    row.kind === "directory" &&
    !excluded(row) &&
    !collected(row) &&
    priority.canRequest(row.item) &&
    !priority.requested(row.item)
  );
}

/**
 * Whether the row would offer "Analyze first" but belongs to an earlier result
 * of the running scan, so the action stays unavailable until the list shows
 * the scan's newest result.
 */
export function waitsForNewerResult(
  row: FolderQueryRow,
  priority: ScanPriority | undefined,
  readOnly: boolean,
): boolean {
  return (
    priority !== undefined &&
    !readOnly &&
    row.kind === "directory" &&
    !excluded(row) &&
    !collected(row) &&
    priority.superseded(row.item)
  );
}

/** Why "Analyze first" is unavailable while the list shows an earlier result. */
export function newerResultReason(text: LocaleText): string {
  return text(
    "ui.browse.analyze_first_newer_result",
    "A newer result is available · Switch to it to analyze a folder first",
  );
}

/** Why one selected row cannot be analyzed first, when the row itself is the reason. */
function rowReason(
  row: FolderQueryRow,
  priority: ScanPriority,
  text: LocaleText,
): string | undefined {
  if (row.kind !== "directory")
    return text(
      "ui.browse.analyze_first_folders_only",
      "Only folders can be analyzed first",
    );
  if (excluded(row))
    return text(
      "ui.browse.analyze_first_excluded",
      "Excluded folders cannot be analyzed first",
    );
  if (collected(row))
    return text(
      "ui.browse.analyze_first_collected",
      "This folder is already fully analyzed",
    );
  if (priority.requested(row.item))
    return text("ui.browse.analyze_first_requested", "Already requested");
  return undefined;
}

function selectedRow(
  pages: BrowsePages,
  selection: Selection,
): FolderQueryRow | undefined {
  if (selection === "all") return pages.count === 1 ? pages.row(0) : undefined;
  if (selection.size !== 1) return undefined;
  const [key] = selection;
  return pages.findRow((candidate) => rowKey(candidate.item) === key);
}

/** The selection count, followed by why "Analyze first" is unavailable for it. */
function summaryText(
  count: number,
  row: FolderQueryRow | undefined,
  priority: ScanPriority,
  text: LocaleText,
): string {
  if (count === 0)
    return text(
      "ui.browse.no_selection",
      "Select one folder to analyze it first",
    );
  const selected =
    count === 1
      ? text("ui.browse.selected_one", "1 item selected")
      : text("ui.browse.selected_count", "{count} items selected", {
          count: BigInt(count),
        });
  const reason =
    count > 1
      ? text(
          "ui.browse.analyze_first_one_folder",
          "Select one folder to analyze it first",
        )
      : row && rowReason(row, priority, text);
  return reason ? `${selected} · ${reason}` : selected;
}

/**
 * The selection action bar offers "Analyze first" while the list shows the
 * running scan's current result. It acts only on one selected folder that the
 * row action would also offer; otherwise the summary says why.
 *
 * While the list shows an earlier result of the running scan, the bar stays
 * with "Analyze first" unavailable and its summary says that a newer result
 * is available, ahead of any reason about the selection. When the host can
 * switch the list, the bar also offers "Show latest result".
 *
 * An unavailable action stays focusable through `aria-disabled` and refers to
 * the summary for its reason, so pressing it by keyboard keeps focus in the
 * bar while the folder becomes requested. If the bar goes away while it holds
 * focus, focus moves to the panel.
 *
 * The bar also shows the outcome of the last request in place of its summary
 * for as long as the scan priority state offers it, and stays while it does,
 * so an outcome such as "The analysis has finished." is not removed with the
 * bar when the scan ends.
 */
export function SelectionAnalyzeFirst({
  pages,
  selection,
  priority,
  readOnly,
  focusPanel,
}: {
  pages: BrowsePages;
  selection: Selection;
  priority: ScanPriority | undefined;
  readOnly: boolean;
  focusPanel: () => void;
}) {
  const text = useLocaleText();
  const summaryId = useId();
  const bar = useRef<HTMLDivElement>(null);
  const holdsFocus = useRef(false);
  const first = pages.row(0);
  const notice = priority?.notice;
  const behind =
    first !== undefined && priority?.superseded(first.item) === true;
  const visible =
    priority !== undefined &&
    !readOnly &&
    first !== undefined &&
    (priority.canRequest(first.item) || behind || notice !== undefined);
  useLayoutEffect(() => {
    function track(event: FocusEvent) {
      holdsFocus.current =
        event.target instanceof Node &&
        (bar.current?.contains(event.target) ?? false);
    }
    document.addEventListener("focusin", track);
    return () => document.removeEventListener("focusin", track);
  }, []);
  useLayoutEffect(() => {
    if (visible || !holdsFocus.current) return;
    holdsFocus.current = false;
    if (document.activeElement === document.body) focusPanel();
  }, [visible, focusPanel]);
  if (!visible || !priority) return null;
  const count = selection === "all" ? pages.count : selection.size;
  const row = selectedRow(pages, selection);
  const target =
    row && canAnalyzeFirst(row, priority, readOnly) ? row : undefined;
  const showLatest = behind ? priority.showLatest : undefined;
  return (
    <SelectionActions
      ref={bar}
      label={text("ui.browse.selection_actions", "Selection actions")}
      summary={
        behind
          ? newerResultReason(text)
          : summaryText(count, row, priority, text)
      }
      summaryId={summaryId}
      notice={
        notice && {
          id: notice.id,
          tone: scanPriorityTone(notice.outcome),
          // The notice text is a string, so the marks are written into it and
          // the spoken explanation is left out; the request keeps the real name.
          text: scanPriorityText(
            {
              ...notice,
              name: markNameEdges(notice.name),
              previous:
                notice.previous === undefined
                  ? undefined
                  : markNameEdges(notice.previous),
            },
            text,
          ),
        }
      }
    >
      {showLatest && (
        <Action onPress={showLatest}>
          {text("ui.analysis.show_latest", "Show latest result")}
        </Action>
      )}
      <Action
        {...(target === undefined
          ? { "aria-disabled": true, "aria-describedby": summaryId }
          : {})}
        onPress={() => {
          if (target)
            priority.request(
              target.item,
              browseDisplayName(target.displayName, text),
            );
        }}
      >
        {text("ui.browse.analyze_first", "Analyze first")}
      </Action>
    </SelectionActions>
  );
}
