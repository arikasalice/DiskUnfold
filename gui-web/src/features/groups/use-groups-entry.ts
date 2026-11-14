import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ItemRef } from "../../ipc";

/** The part of a browse answer the virtual-group view needs. */
type BrowsedLocation = {
  rootKind: string;
  location: ItemRef | null;
  page: { resultVersion: string };
};

/** The folder a browse result shows; the virtual root stands for the whole result. */
function browsedFolder(opened: BrowsedLocation): ItemRef | undefined {
  const { location, page, rootKind } = opened;
  if (rootKind === "virtual_root" || !location) return undefined;
  return location.resultVersion === page.resultVersion ? location : undefined;
}

/**
 * State the app shell keeps for the virtual-group screen: whether it is open,
 * the folder it classifies, and the entry button that gets focus back on close.
 */
export function useGroupsEntry(browseVersion: string | undefined) {
  const [open, setOpen] = useState(false);
  const [browsed, setBrowsed] = useState<{
    version: string;
    scope: ItemRef | undefined;
  }>();
  const trigger = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  useLayoutEffect(() => {
    if (!open && restoreFocus.current) {
      trigger.current?.focus();
      restoreFocus.current = false;
    }
  }, [open]);
  useEffect(() => {
    if (!browseVersion) setOpen(false);
  }, [browseVersion]);
  const known =
    browseVersion !== undefined && browsed?.version === browseVersion;
  return {
    open,
    trigger,
    /** The folder the open screen classifies; undefined means the whole result. */
    scope: known ? browsed?.scope : undefined,
    /** The entry button shows once the browse screen reports its folder. */
    entryVisible: (blocked: boolean) => known && !blocked && !open,
    onBrowsed: (version: string, opened: BrowsedLocation) =>
      setBrowsed({ version, scope: browsedFolder(opened) }),
    show: () => setOpen(true),
    /** True while closing hands focus back to the entry button. */
    restoringFocus: () => restoreFocus.current,
    close: () => {
      restoreFocus.current = true;
      setOpen(false);
    },
  };
}

/** The state object returned by {@link useGroupsEntry}. */
export type GroupsEntry = ReturnType<typeof useGroupsEntry>;
