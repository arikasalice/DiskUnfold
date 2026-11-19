import { useCallback, useEffect, useRef } from "react";
import type { SizeDisplay } from "../i18n/bytes";
import type { SizeBasisFilter } from "../features/analysis/space-format";
import type { BrowseOpenLocation } from "../features/browse/BrowsePanel";
import { browseSelectionFromOpen } from "./browse-visualization";
import type { BrowseVisualizationSelection } from "./browse-visualization";

/** The size basis the analysis summary reported for the result it shows. */
export type ReportedBasis = { version: string; basis: SizeBasisFilter };

/**
 * Hands an opened folder to the space view once the analysis summary has
 * reported the size basis of the same result. Right after a scan ends, the
 * folder list can open a newer result than the summary still shows; the view
 * then becomes available when the summary catches up, without opening the
 * folder again. Starting another opening forgets the folder.
 */
export function useVisualizationOffer(
  reported: ReportedBasis | undefined,
  display: SizeDisplay,
  opened: (selection: BrowseVisualizationSelection | undefined) => void,
) {
  const pending = useRef<{ version: string; location: BrowseOpenLocation }>(
    undefined,
  );
  const offer = useCallback(() => {
    const folder = pending.current;
    if (!folder || reported?.version !== folder.version) return false;
    pending.current = undefined;
    opened(
      browseSelectionFromOpen(
        folder.version,
        folder.location,
        display,
        reported.basis,
      ),
    );
    return true;
  }, [reported, display, opened]);
  useEffect(() => {
    offer();
  }, [offer]);
  const opening = useCallback(() => {
    pending.current = undefined;
  }, []);
  const browsed = useCallback(
    (version: string, location: BrowseOpenLocation) => {
      pending.current = { version, location };
      if (!offer()) opened(undefined);
    },
    [offer, opened],
  );
  return {
    /** Forgets the folder whose list is being replaced. */
    opening,
    /** Records the opened folder and offers it once its basis is reported. */
    browsed,
  };
}
