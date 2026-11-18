import type { VisualizationMessageKey } from "./messages";
import type { VizText } from "./model-text";

/**
 * Wire words and keys that a model carries for its status line, evidence table
 * and legend, with the message that names each one. Only words listed here are
 * translated; the table is the whole vocabulary the producer uses today.
 */
const keyMessages: Readonly<Record<string, VisualizationMessageKey>> = {
  complete: "completenessComplete",
  partial: "completenessPartial",
  missing: "completenessMissing",
  cancelled: "completenessCancelled",
  current: "freshnessCurrent",
  stale: "freshnessStale",
  unknown: "freshnessUnknown",
  scan: "originScan",
  current_analysis: "originScan",
  scope: "denominatorScope",
  parent_folder: "denominatorParentFolder",
  analysis_total: "denominatorAnalysisTotal",
  logical: "sizeBasisLogical",
  allocated: "sizeBasisAllocated",
  unique_physical: "sizeBasisUniquePhysical",
  size: "measureSize",
  treemap: "treemap",
  bar: "bar",
  trend: "trend",
  waterfall: "waterfall",
  histogram: "histogram",
  sankey: "sankey",
  "viz.flag.unknown": "flagUnknown",
  "viz.flag.lower_bound": "flagLowerBound",
  "viz.flag.other_group": "flagOtherGroup",
  unclassified: "legend.unclassified",
  "ui.visualization.group.other": "otherItems",
  "ui.visualization.legend.unclassified": "legend.unclassified",
};

/**
 * Names a wire word or key in the active language. A word outside the table gets
 * the general phrase, so neither a raw key nor a missing-translation marker
 * reaches the screen.
 */
export function createVizKeyText(message: VizText["message"]): VizText["key"] {
  return (key) => {
    const known = Object.hasOwn(keyMessages, key)
      ? keyMessages[key]
      : undefined;
    return message(known ?? "unrecognizedValue");
  };
}
