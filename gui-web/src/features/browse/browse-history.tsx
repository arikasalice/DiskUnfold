import type { Dispatch, SetStateAction } from "react";
import { ActionWithTooltip } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { BrowseOpenResponse } from "../../ipc";
import { BrowsePages } from "../../state/browse-pages";
import { browseDisplayName } from "./browse-display";
import type { InspectedItem } from "./use-item-inspection";

export type Opened = Extract<BrowseOpenResponse, { kind: "opened" }>;

export type PreviousView = {
  opened: Opened;
  pages: BrowsePages;
  restoreInspection?: InspectedItem | undefined;
};

export type View = {
  opened: Opened;
  pages: BrowsePages;
  previous?: PreviousView;
};

export function priorView(
  current: View | undefined,
  resultVersion: string,
  location: { resultVersion: string; itemNo: number } | undefined,
  retrying: boolean,
  inspection: InspectedItem | undefined,
  rememberCurrent: boolean,
): PreviousView | undefined {
  if (!rememberCurrent) return undefined;
  if (!current || retrying) return undefined;
  if (current.opened.page.resultVersion !== resultVersion) return undefined;
  if ((current.opened.location?.itemNo ?? null) === (location?.itemNo ?? null))
    return undefined;
  return {
    opened: current.opened,
    pages: current.pages,
    ...(inspection ? { restoreInspection: inspection } : {}),
  };
}

function previousFolderPath(
  previous: PreviousView,
  text: ReturnType<typeof useLocaleText>,
): string {
  const inspected = previous.restoreInspection;
  if (!inspected) return "";
  const labels = [
    ...previous.opened.breadcrumbs.map((crumb) =>
      browseDisplayName(crumb.displayName, text),
    ),
    inspected.name,
  ];
  return inspected.countedAt?.encoding === "windows_utf16le"
    ? labels.join("\\")
    : `/${labels.join("/")}`;
}

export function restorePrevious(
  current: View | undefined,
  setView: Dispatch<SetStateAction<View | undefined>>,
  restore: (item: InspectedItem) => void,
  reset: () => void,
  busy: boolean,
) {
  const previous = current?.previous;
  if (!previous || busy) return;
  setView(previous);
  if (previous.restoreInspection)
    restore({ ...previous.restoreInspection, focusCountedAtItem: true });
  else reset();
}

export function PreviousFolderAction({
  previous,
  onPress,
  text,
}: {
  previous: PreviousView | undefined;
  onPress: () => void;
  text: ReturnType<typeof useLocaleText>;
}) {
  if (!previous) return null;
  return (
    <ActionWithTooltip
      tooltip={previousFolderPath(previous, text)}
      variant="quiet"
      icon="back"
      onPress={onPress}
    >
      {text("ui.browse.previous_location", "Back to previous folder")}
    </ActionWithTooltip>
  );
}
