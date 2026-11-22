import { useEffect } from "react";
import type { MutableRefObject, RefObject } from "react";
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
  /** The list was just opened at a saved position: it puts the focus there once. */
  focusRestored?: boolean;
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

/** The saved selection of a list opened at a saved position, else none. */
export function resumedKeys(resume: BrowseResumeRef | undefined): string[] {
  return resume?.current?.selected ?? [];
}

/** Keeps the selection up to date in the saved position. */
export function rememberSelection(
  resume: BrowseResumeRef | undefined,
  next: "all" | ReadonlySet<string>,
) {
  if (resume?.current)
    resume.current.selected = next === "all" ? [] : [...next].map(String);
}

/**
 * Puts the saved scroll position back when the rows mount and keeps it up to date as
 * they scroll; a list opened anew has none saved (#406).
 */
export function useResumedScroll(
  resume: BrowseResumeRef | undefined,
  rows: RefObject<HTMLElement | null>,
) {
  useEffect(() => {
    const root = rows.current;
    if (!root || !resume) return;
    if (resume.current?.scrollTop) root.scrollTop = resume.current.scrollTop;
    const save = () => {
      if (resume.current) resume.current.scrollTop = root.scrollTop;
    };
    root.addEventListener("scroll", save, { passive: true });
    return () => root.removeEventListener("scroll", save);
    // The list is remounted for every folder, so this reads the saved position once.
  }, []);
}
