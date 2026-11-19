import { createVizKeyText } from "./key-text";
import type { VisualizationMessageKey } from "./messages";
import { vizMessage } from "./model-text";
import type { VizText } from "./model-text";

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
  "viz.unavailable.basis_not_counted": {
    title: "unavailableTitleBasisNotCounted",
    body: "unavailableBodyBasisNotCounted",
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

/** What the answer says besides its reason, and what the chart was asked for. */
export type UnavailableContext = {
  /** The answer's result completeness; `partial` while a scan is still running. */
  completeness?: string | undefined;
  /** The size basis of the request, as written on the wire. */
  sizeBasis?: string | undefined;
};

/**
 * The title (what is missing) and body (why, and what to do) for an unavailable
 * reason. Missing item sizes promise to appear only while a scan is running;
 * a finished result says only that none is confirmed. A basis without
 * evidence names the basis that was asked for.
 */
export function visualizationUnavailableText(
  reasonKey: string,
  message: VizText["message"],
  context: UnavailableContext = {},
): { title: string; body: string } {
  const words = Object.hasOwn(unavailableWords, reasonKey)
    ? unavailableWords[reasonKey]!
    : unknownWords;
  const body =
    words.body === "unavailableBodyNoItemSizes" &&
    context.completeness !== "partial"
      ? "unavailableBodyNoItemSizesFinished"
      : words.body;
  const basis = createVizKeyText(message)(context.sizeBasis ?? "");
  return {
    title: message(words.title),
    body: vizMessage({ message }, body, { basis }),
  };
}
