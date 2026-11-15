import { RowAction, RowSlot, StatusBadge } from "../../components";
import type { ItemAction } from "../../components";
import type { FolderQueryRow } from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { ScanPriority } from "../../state/scan-priority";
import { browseDisplayName } from "./browse-display";
import {
  canAnalyzeFirst,
  newerResultReason,
  waitsForNewerResult,
} from "./SelectionAnalyzeFirst";

/**
 * "Analyze first" in the row menu: available for a folder of the running
 * scan's current result, and unavailable with its reason while the list shows
 * an earlier result of that scan.
 */
export function analyzeFirstActions(
  row: FolderQueryRow,
  priority: ScanPriority | undefined,
  readOnly: boolean,
  text: LocaleText,
): ItemAction[] {
  const label = text("ui.browse.analyze_first", "Analyze first");
  if (waitsForNewerResult(row, priority, readOnly))
    return [
      {
        id: "analyze-first",
        label,
        onAction: () => undefined,
        disabledReason: newerResultReason(text),
      },
    ];
  if (!canAnalyzeFirst(row, priority, readOnly)) return [];
  return [
    {
      id: "analyze-first",
      label,
      onAction: () =>
        priority.request(row.item, browseDisplayName(row.displayName, text)),
    },
  ];
}

/**
 * Whether the list shows a result of a running or paused scan, where folders
 * can be analyzed first. Every row of a list shares its result, so every row
 * gives the same answer.
 */
function offersPriority(
  row: FolderQueryRow,
  priority: ScanPriority | undefined,
): priority is ScanPriority {
  return (
    priority !== undefined &&
    (priority.canRequest(row.item) ||
      priority.superseded(row.item) ||
      priority.status(row.item) !== undefined)
  );
}

/**
 * The end of a row's name while the list shows a running or paused scan's
 * result: the requested folder's badge, else "Analyze first" for a folder
 * that offers it. A requested folder offers no action; it was already asked.
 * A finished result keeps no place, so the name gets its width.
 */
export function PrioritySlot({
  row,
  priority,
  readOnly,
}: {
  row: FolderQueryRow;
  priority: ScanPriority | undefined;
  readOnly: boolean;
}) {
  const text = useLocaleText();
  if (!offersPriority(row, priority)) return null;
  const requested = text(
    "ui.browse.priority_requested",
    "Analyze first requested",
  );
  const analyzing = text("ui.browse.priority_analyzing", "Analyzing first");
  const action = text("ui.browse.analyze_first", "Analyze first");
  const status =
    row.kind === "directory" ? priority.status(row.item) : undefined;
  const offered = !status && canAnalyzeFirst(row, priority, readOnly);
  return (
    <RowSlot
      badges={[requested, analyzing]}
      actions={[action]}
      holds={status ? "badge" : offered ? "action" : undefined}
    >
      {status ? (
        <StatusBadge tone="info">
          {status === "analyzing" ? analyzing : requested}
        </StatusBadge>
      ) : offered ? (
        <RowAction
          label={action}
          onPress={() =>
            priority.request(row.item, browseDisplayName(row.displayName, text))
          }
        />
      ) : null}
    </RowSlot>
  );
}
