import { useId } from "react";
import { ActionWithTooltip, MiddleEllipsisText, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { ScanIssueFolder } from "./scan-issue-filter";

/**
 * The chip that tells the issue list is restricted to one folder and everything below
 * it, and clears that restriction when pressed (F02 #358).
 */
export function ScanIssueFolderChip({
  folder,
  onClear,
}: {
  folder: ScanIssueFolder;
  onClear: () => void;
}) {
  const text = useLocaleText();
  const pathId = useId();
  const path = folder.path ?? folder.label;
  return (
    <div className="du-scan-issues__chip">
      <ActionWithTooltip
        aria-label={text(
          "ui.analysis.issues_filter_folder_name",
          "Location: {folder} and below, clear location filter",
          { folder: folder.label },
        )}
        aria-describedby={pathId}
        tooltip={path}
        onPress={onClear}
      >
        <Text inline>
          {text("ui.analysis.issues_filter_folder", "Location:")}
        </Text>
        <Text inline className="du-scan-issues__storage-path">
          <MiddleEllipsisText text={folder.label} cell />
        </Text>
        <Text inline>
          {text("ui.analysis.issues_filter_folder_below", "and below")}
        </Text>
        <Text inline>×</Text>
      </ActionWithTooltip>
      <div id={pathId} className="du-visually-hidden">
        {path}
      </div>
    </div>
  );
}
