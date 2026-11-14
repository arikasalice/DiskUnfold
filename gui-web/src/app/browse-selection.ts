import { useState } from "react";
import type { SizeDisplay } from "../i18n/bytes";
import type { ScanPriorityView } from "../state/scan-priority";

/**
 * The result the folder list shows, its size display choice, and the scan
 * that published it when the analysis panel opened it from that scan.
 *
 * The analysis panel names the scan whose total it offered, so a selection
 * pairs a result only with the scan that published it.
 */
export type BrowseSelection = {
  version: string;
  choice: SizeDisplay | undefined;
  scan: string | undefined;
};

/**
 * Holds the folder list's selection. `version` and `choice` are undefined
 * while no list is open.
 */
export function useBrowseSelection() {
  const [selection, setSelection] = useState<BrowseSelection>();
  return {
    selection,
    version: selection?.version,
    choice: selection?.choice,
    /** Replaces the selection; undefined closes the list. */
    set: setSelection,
  };
}

/**
 * Returns the single opening path for the folder list. The analysis panel's
 * "Browse folders" and the list's "Show latest result" both use it.
 * `invalidate` drops the space view of the list being replaced before the new
 * selection renders.
 */
export function browseOpener(
  invalidate: () => void,
  set: (selection: BrowseSelection) => void,
): (selection: BrowseSelection) => void {
  return (selection) => {
    invalidate();
    set(selection);
  };
}

/**
 * Names the scan of the listed result for "Analyze first", so an earlier
 * result of the running scan can offer its newest one through the same
 * opening path. A list without a known scan passes no view.
 */
export function priorityView(
  selection: BrowseSelection | undefined,
  open: (selection: BrowseSelection) => void,
): ScanPriorityView | undefined {
  if (!selection?.scan) return undefined;
  const { choice, scan } = selection;
  return {
    jobRef: scan,
    resultVersion: selection.version,
    open: (version) => open({ version, choice, scan }),
  };
}
