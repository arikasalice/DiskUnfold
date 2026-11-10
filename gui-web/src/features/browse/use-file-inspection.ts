import { useEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import type { FolderQueryRow, ItemRef } from "../../ipc";
import { browseDisplayName } from "./browse-display";

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
      if (active !== document.body && !panel.current?.contains(active)) return;
      const target = restore.opener?.isConnected
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
