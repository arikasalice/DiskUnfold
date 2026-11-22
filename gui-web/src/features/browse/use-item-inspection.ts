import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { FolderQueryRow, ItemRef, OsPath } from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import { browseDisplayName, rowKey } from "./browse-display";
import { hasUnreadPart, scanStatus } from "./browse-cells";

export type InspectedItem = {
  item: ItemRef;
  kind: "file" | "folder";
  name: string;
  countedAt?: OsPath;
  countedAtItem?: ItemRef;
  alreadyCounted?: boolean;
  groupVolumeRole?: string;
  focusCountedAtItem?: boolean;
  opener: HTMLElement | null;
  /** The opener's position in the list. */
  index: number;
  /** The row's scan status when something in it was not read, else undefined. */
  unreadNote: string | undefined;
};

/**
 * Holds an observed item and its return target for the current browse
 * revision. When the opener's row went away while the details were open,
 * focusRow receives the row's former position.
 */
export function useItemInspection(
  version: string | undefined,
  focusRow: (index: number) => void,
) {
  const text = useLocaleText();
  const [item, setItem] = useState<InspectedItem>();
  const pending = useRef<InspectedItem>(undefined);
  const fallback = useRef(focusRow);
  useLayoutEffect(() => {
    fallback.current = focusRow;
  });
  const selected = item?.item.resultVersion === version ? item : undefined;

  useEffect(() => {
    const restore = pending.current;
    if (selected || !restore) return;
    pending.current = undefined;
    if (restore.item.resultVersion !== version) return;
    const frame = requestAnimationFrame(() => {
      const active = document.activeElement;
      // React Aria owns normal modal restoration. Only recover an orphaned
      // focus, never override a control the user or the modal already focused.
      if (active !== document.body) return;
      if (
        restore.opener?.isConnected &&
        restore.opener.dataset.itemKey === rowKey(restore.item)
      )
        restore.opener.focus({ preventScroll: true });
      else fallback.current(restore.index);
    });
    return () => cancelAnimationFrame(frame);
  }, [selected, version]);

  return {
    selected,
    open(row: FolderQueryRow, trigger: Element | null) {
      if (
        row.item.resultVersion !== version ||
        (row.kind !== "file" && row.kind !== "directory")
      )
        return;
      const opener = trigger?.closest<HTMLElement>('[role="row"]') ?? null;
      if (opener?.dataset.itemKey !== rowKey(row.item)) return;
      // Normalize pointer, keyboard and accessibility activation before the
      // modal captures its return target. Return to the navigable row, not
      // its nested button (which is deliberately outside the tab order).
      opener.focus({ preventScroll: true });
      const alreadyCounted =
        row.flags.boundary?.state === "present" &&
        row.flags.boundary.value === "already_counted";
      const countedAt = alreadyCounted ? row.flags.countedAt : undefined;
      setItem({
        item: row.item,
        kind: row.kind === "directory" ? "folder" : "file",
        name: browseDisplayName(row.displayName, text),
        ...(countedAt
          ? {
              countedAt,
              ...(row.flags.countedAtItem
                ? { countedAtItem: row.flags.countedAtItem }
                : {}),
            }
          : {}),
        ...(alreadyCounted ? { alreadyCounted: true } : {}),
        ...(typeof row.flags.groupVolumeRole === "string"
          ? { groupVolumeRole: row.flags.groupVolumeRole }
          : {}),
        opener,
        index: Number(opener.dataset.row ?? 0),
        unreadNote: hasUnreadPart(row) ? scanStatus(row, text) : undefined,
      });
    },
    close() {
      pending.current = selected;
      setItem(undefined);
    },
    restore(item: InspectedItem) {
      pending.current = undefined;
      setItem(item);
    },
    reset() {
      pending.current = undefined;
      setItem(undefined);
    },
  };
}
