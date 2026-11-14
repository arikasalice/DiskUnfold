import type { LocaleBundleBody } from "../../ipc";

/**
 * True when the selected language bundle lacks a message that the baseline
 * bundle has (F13 FR12.HD1).
 *
 * A missing message is displayed from the baseline language, so the user can
 * see mixed languages. The comparison looks at key presence only; it cannot
 * judge the quality of a translation. The same bundle object is always
 * complete.
 */
export function translationIncomplete(
  selected: LocaleBundleBody,
  baseline: LocaleBundleBody,
): boolean {
  if (selected === baseline) return false;
  return Object.keys(baseline.messages).some(
    (key) => !Object.hasOwn(selected.messages, key),
  );
}
