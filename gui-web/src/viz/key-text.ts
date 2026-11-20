import type { VisualizationMessageKey } from "./messages";

/** A localized message, as VizText gives it. */
type Message = (
  key: VisualizationMessageKey,
  values?: Readonly<Record<string, string | bigint>>,
) => string;

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
  count: "measureCount",
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
  "ui.visualization.group.other": "group.other",
  "ui.visualization.legend.unclassified": "legend.unclassified",
  "ui.visualization.legend.type.document": "legend.type.document",
  "ui.visualization.legend.type.audio": "legend.type.audio",
  "ui.visualization.legend.type.image": "legend.type.image",
  "ui.visualization.legend.type.installer_image": "legend.type.installer_image",
  "ui.visualization.legend.type.video": "legend.type.video",
  "ui.visualization.legend.type.archive": "legend.type.archive",
  "ui.visualization.legend.type.unclassified": "legend.type.unclassified",
  "ui.visualization.legend.type.no_extension": "legend.type.no_extension",
  "ui.visualization.legend.type.unavailable": "legend.type.unavailable",
};

/**
 * Names a wire word or key in the active language. A word outside the table gets
 * the general phrase, so neither a raw key nor a missing-translation marker
 * reaches the screen.
 */
export function createVizKeyText(message: Message): (key: string) => string {
  return (key) => {
    const known = Object.hasOwn(keyMessages, key)
      ? keyMessages[key]
      : undefined;
    return message(known ?? "unrecognizedValue");
  };
}
