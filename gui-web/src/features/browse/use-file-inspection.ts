import { useEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import type { FolderQueryRow, ItemRef } from "../../ipc";
import { browseDisplayName, rowKey } from "./browse-display";

export type InspectedFile = {
  item: ItemRef;
  name: string;
  opener: HTMLElement | null;
};

/** Holds a file and its return target only for the current browse revision. */
export function useFileInspection(
  version: string | undefined,
  panel: RefObject<HTMLElement | null>,
) {
  const [file, setFile] = useState<InspectedFile>();
  const pending = useRef<InspectedFile>(undefined);
  const selected = file?.item.resultVersion === version ? file : undefined;

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
      if (row.item.resultVersion !== version) return;
      const opener = trigger?.closest<HTMLElement>('[role="row"]') ?? null;
      if (opener?.dataset.itemKey !== rowKey(row.item)) return;
      // Normalize pointer, keyboard and accessibility activation before the
      // modal captures its return target. Return to the navigable row, not
      // its nested button (which is deliberately outside the tab order).
      opener.focus({ preventScroll: true });
      setFile({
        item: row.item,
        name: browseDisplayName(row.displayName),
        opener,
      });
    },
    close() {
      pending.current = selected;
      setFile(undefined);
    },
    reset() {
      pending.current = undefined;
      setFile(undefined);
    },
  };
}
