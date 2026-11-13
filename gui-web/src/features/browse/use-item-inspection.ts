import { useEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import type { FolderQueryRow, ItemRef } from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import { browseDisplayName, rowKey } from "./browse-display";

export type InspectedItem = {
  item: ItemRef;
  kind: "file" | "folder";
  name: string;
  opener: HTMLElement | null;
};

/** Holds an observed item and its return target for the current browse revision. */
export function useItemInspection(
  version: string | undefined,
  panel: RefObject<HTMLElement | null>,
) {
  const text = useLocaleText();
  const [item, setItem] = useState<InspectedItem>();
  const pending = useRef<InspectedItem>(undefined);
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
      const target =
        restore.opener?.isConnected &&
        restore.opener.dataset.itemKey === rowKey(restore.item)
          ? restore.opener
          : panel.current;
      target?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [selected, version, panel]);

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
      setItem({
        item: row.item,
        kind: row.kind === "directory" ? "folder" : "file",
        name: browseDisplayName(row.displayName, text),
        opener,
      });
    },
    close() {
      pending.current = selected;
      setItem(undefined);
    },
    reset() {
      pending.current = undefined;
      setItem(undefined);
    },
  };
}
