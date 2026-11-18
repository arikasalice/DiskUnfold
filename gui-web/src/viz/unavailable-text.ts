import type { VisualizationMessageKey } from "./messages";

type Words = { title: VisualizationMessageKey; body: VisualizationMessageKey };

/**
 * Title and explanation for each reason the engine gives for a chart it cannot
 * draw. A reason outside this table, including one the screen has never seen,
 * gets the general pair. `viz.unavailable.no_stable_order` is not produced by
 * the engine and has no entry.
 */
const unavailableWords: Readonly<Record<string, Words>> = {
  "viz.unavailable.no_baseline": {
    title: "unavailableTitleNoBaseline",
    body: "unavailableBodyNoBaseline",
  },
  "viz.unavailable.single_observation": {
    title: "unavailableTitleSingleObservation",
    body: "unavailableBodySingleObservation",
  },
  "viz.unavailable.no_attribution": {
    title: "unavailableTitleNoAttribution",
    body: "unavailableBodyNoAttribution",
  },
  "viz.unavailable.no_item_sizes": {
    title: "unavailableTitleNoItemSizes",
    body: "unavailableBodyNoItemSizes",
  },
  "viz.unavailable.negative_flow": {
    title: "unavailableTitleNegativeFlow",
    body: "unavailableBodyNegativeFlow",
  },
};

const unknownWords: Words = {
  title: "unavailableTitleUnknown",
  body: "unavailableBodyUnknown",
};

/** The title (what is missing) and body (why, and what to do) for an unavailable reason. */
export function visualizationUnavailableText(
  reasonKey: string,
  message: (key: VisualizationMessageKey) => string,
): { title: string; body: string } {
  const words = Object.hasOwn(unavailableWords, reasonKey)
    ? unavailableWords[reasonKey]!
    : unknownWords;
  return { title: message(words.title), body: message(words.body) };
}
