import type { MutableRefObject } from "react";
import type { ItemRef } from "../../ipc";

/**
 * Where the user was in the folder list, kept by the host while the list is closed
 * (F02 #406): the pinned result, the open folder (undefined at the scope roots), the
 * selected rows by row key and the scroll position of the rows. The list reads it when
 * it opens the same result again and writes it as the user moves.
 */
export type BrowseResume = {
  version: string;
  location: ItemRef | undefined;
  selected: string[];
  scrollTop: number;
};

export type BrowseResumeRef = MutableRefObject<BrowseResume | undefined>;

/** Whether a saved position is of this result and folder. */
export function resumeMatches(
  saved: BrowseResume | undefined,
  version: string,
  location: ItemRef | undefined,
): saved is BrowseResume {
  return (
    saved !== undefined &&
    saved.version === version &&
    saved.location?.itemNo === location?.itemNo &&
    saved.location?.resultVersion === location?.resultVersion
  );
}
