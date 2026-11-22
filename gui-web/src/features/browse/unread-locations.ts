import type { ItemAction } from "../../components";
import type { FolderQueryRow, ItemRef } from "../../ipc";
import type { LocaleText } from "../../i18n/presentation";
import { browseDisplayName } from "./browse-display";
import { hasUnreadPart } from "./browse-cells";

/**
 * "View unread locations" in the row menu of a folder with an unread part: it opens the
 * scan issue list restricted to that folder and everything below it (F02 #358).
 */
export function unreadLocationActions(
  row: FolderQueryRow,
  viewUnread: ((item: ItemRef, name: string) => void) | undefined,
  text: LocaleText,
): ItemAction[] {
  if (!viewUnread || row.kind !== "directory" || !hasUnreadPart(row)) return [];
  return [
    {
      id: "view-unread",
      label: text("ui.browse.view_unread_locations", "View unread locations"),
      onAction: () =>
        viewUnread(row.item, browseDisplayName(row.displayName, text)),
    },
  ];
}
