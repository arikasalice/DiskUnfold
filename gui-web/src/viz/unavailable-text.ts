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
 * reason. Missing item sizes are explained only while a scan is running; for a
 * finished result the title says it all and there is no body. A basis without
 * evidence names the basis that was asked for.
 */
export function visualizationUnavailableText(
  reasonKey: string,
  message: VizText["message"],
  context: UnavailableContext = {},
): { title: string; body: string | null } {
  const words = Object.hasOwn(unavailableWords, reasonKey)
    ? unavailableWords[reasonKey]!
    : unknownWords;
  const title = message(words.title);
  if (
    words.body === "unavailableBodyNoItemSizes" &&
    context.completeness !== "partial"
  )
    return { title, body: null };
  const basis = createVizKeyText(message)(context.sizeBasis ?? "");
  return { title, body: vizMessage({ message }, words.body, { basis }) };
}
