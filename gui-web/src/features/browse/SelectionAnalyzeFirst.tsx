import { useId, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode, RefObject } from "react";
import { Action, NameText, SelectionActions } from "../../components";
import { scanPriorityText, scanPriorityTone } from "../../ipc";
import type { FolderQueryRow, ScanPriorityAnnouncement } from "../../ipc";
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
 * Stand-ins passed through the localized sentence in place of the folder
 * names, so the sentence is split exactly where each name stands even when a
 * name repeats or matches a word of the sentence. Translations do not use
 * these private use characters.
 */
const NAME_SLOT = "\uE000";
const PREVIOUS_SLOT = "\uE001";

/**
 * The outcome sentence with each folder name drawn by `NameText`: edge white
 * space is marked on screen only, and the status region reads the real name
 * followed by the explanation of its edge white space. A name without edge
 * white space leaves the sentence as plain text.
 */
function noticeText(
  notice: ScanPriorityAnnouncement,
  text: LocaleText,
): ReactNode {
  const names = new Map([[NAME_SLOT, notice.name]]);
  if (notice.previous !== undefined) names.set(PREVIOUS_SLOT, notice.previous);
  const sentence = scanPriorityText(
    {
      ...notice,
      name: NAME_SLOT,
      previous: notice.previous === undefined ? undefined : PREVIOUS_SLOT,
    },
    text,
  );
  return sentence.split(/([\uE000\uE001])/u).map((part, index) => {
    const name = names.get(part);
    return name === undefined ? part : <NameText key={index} name={name} />;
  });
}

/**
 * Whether the bar shows. It shows for the running scan's current result, an
 * earlier result of that scan or a pending outcome, and stays ("held") while
 * "Show latest result" switches the read-only list to the newest result.
 */
function barState(
  first: FolderQueryRow | undefined,
  priority: ScanPriority | undefined,
  readOnly: boolean,
  switching: boolean,
) {
  const behind =
    first !== undefined && priority?.superseded(first.item) === true;
  const live =
    priority !== undefined &&
    !readOnly &&
    first !== undefined &&
    (priority.canRequest(first.item) ||
      behind ||
      priority.notice !== undefined);
  const held =
    switching && readOnly && priority !== undefined && first !== undefined;
  return { behind, held, visible: live || held };
}

/**
 * Keeps focus out of large areas when the bar comes and goes. A bar that goes
 * away while it holds focus sends it to the list. A bar mounted for a list
 * that replaced one whose bar held focus takes it on "Analyze first", or
 * sends it to the list when it does not show.
 */
function useBarFocus(
  bar: RefObject<HTMLDivElement | null>,
  analyzeFirst: RefObject<HTMLButtonElement | null>,
  barFocus: RefObject<boolean>,
  visible: boolean,
  focusList: () => void,
) {
  const holdsFocus = useRef(false);
  useLayoutEffect(() => {
    function track(event: FocusEvent) {
      holdsFocus.current =
        event.target instanceof Node &&
        (bar.current?.contains(event.target) ?? false);
      barFocus.current = holdsFocus.current;
    }
    document.addEventListener("focusin", track);
    return () => document.removeEventListener("focusin", track);
  }, [bar, barFocus]);
  useLayoutEffect(() => {
    if (!barFocus.current) return;
    barFocus.current = false;
    if (document.activeElement !== document.body) return;
    if (analyzeFirst.current)
      analyzeFirst.current.focus({ preventScroll: true });
    else focusList();
    // Runs once per list: the owner remounts the bar with each new list.
  }, []);
  useLayoutEffect(() => {
    if (visible || !holdsFocus.current) return;
    holdsFocus.current = false;
    barFocus.current = false;
    if (document.activeElement === document.body) focusList();
  }, [visible, focusList, barFocus]);
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
 * focus, focus moves to the list through `focusList`.
 *
 * "Show latest result" keeps the bar while the list switches: both actions
 * stay focusable but unavailable until the newest result replaces the list.
 * When a list replaces the one whose bar held focus, as `barFocus` records
 * across the remount, focus moves to the new bar's "Analyze first", or to the
 * list when the new result shows no bar. No large area takes the focus frame.
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
  focusList,
  barFocus,
}: {
  pages: BrowsePages;
  selection: Selection;
  priority: ScanPriority | undefined;
  readOnly: boolean;
  /** Moves focus to the list when the bar goes away while it holds focus. */
  focusList: () => void;
  /**
   * Whether the bar holds focus, kept by the owner of the list so that it
   * survives the remount of the bar when a new list replaces it.
   */
  barFocus: RefObject<boolean>;
}) {
  const text = useLocaleText();
  const summaryId = useId();
  const bar = useRef<HTMLDivElement>(null);
  const analyzeFirst = useRef<HTMLButtonElement>(null);
  const [switching, setSwitching] = useState(false);
  const first = pages.row(0);
  const notice = priority?.notice;
  const { behind, held, visible } = barState(
    first,
    priority,
    readOnly,
    switching,
  );
  useBarFocus(bar, analyzeFirst, barFocus, visible, focusList);
  if (!visible || !priority) return null;
  const count = selection === "all" ? pages.count : selection.size;
  const row = selectedRow(pages, selection);
  const target =
    row && canAnalyzeFirst(row, priority, readOnly) ? row : undefined;
  const showLatest = behind && !held ? priority.showLatest : undefined;
  return (
    <SelectionActions
      ref={bar}
      label={text("ui.browse.selection_actions", "Selection actions")}
      summary={
        behind || held
          ? newerResultReason(text)
          : summaryText(count, row, priority, text)
      }
      summaryId={summaryId}
      notice={
        notice && {
          id: notice.id,
          tone: scanPriorityTone(notice.outcome),
          text: noticeText(notice, text),
        }
      }
    >
      {(showLatest || held) && (
        <Action
          {...(showLatest === undefined
            ? { "aria-disabled": true, "aria-describedby": summaryId }
            : {})}
          onPress={() => {
            if (!showLatest) return;
            setSwitching(true);
            showLatest();
          }}
        >
          {text("ui.analysis.show_latest", "Show latest result")}
        </Action>
      )}
      <Action
        ref={analyzeFirst}
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
