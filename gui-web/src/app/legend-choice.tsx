import { useState } from "react";
import { SegmentedChoice } from "../components";
import type { VizCatalog } from "../ipc";
import { TYPE_CRITERION } from "../viz";
import type { VisualizationViewRequest, VizText } from "../viz";

const NONE = "none";
/** The legend column and the gap before it, in CSS px (README 2). */
const LEGEND_SPACE = 240 + 16;
/** From this chart area width the legend stands beside the chart (README 2). */
const LEGEND_BESIDE = 720;

/**
 * The colour criterion and the legend of the space view (F06 README 44.1).
 *
 * The treemap and the bar chart start coloured by type when the catalog serves
 * it; the choice and the pattern switch are kept while the screen is used and
 * every folder shares them, and a restart starts on type again. A histogram
 * has one colour and no legend. The legend stands beside a chart area of
 * 720 px or more, which then asks for a chart narrower by the legend column.
 */
export function useLegendChoice(
  format: string,
  catalog: VizCatalog | undefined,
  owner:
    { request: Pick<VisualizationViewRequest, "colorCriterion"> } | undefined,
  text: VizText,
) {
  const [criterion, setCriterion] = useState(TYPE_CRITERION);
  const [patterns, setPatterns] = useState(false);
  const ownerCriterion = owner?.request.colorCriterion ?? NONE;
  if (
    format === "histogram" ||
    !catalog?.colorCriteria.includes(TYPE_CRITERION)
  )
    return {
      colorCriterion: ownerCriterion,
      controls: undefined,
      side: () => undefined,
      chartWidth: (width: number) => width,
    };
  const beside = (width: number) => width >= LEGEND_BESIDE;
  return {
    colorCriterion: criterion === TYPE_CRITERION ? TYPE_CRITERION : NONE,
    controls: {
      criterion: (
        <SegmentedChoice
          label={text.message("colorBy")}
          value={criterion}
          onChange={setCriterion}
          options={[
            { value: NONE, label: text.message("colorNone") },
            { value: TYPE_CRITERION, label: text.message("colorType") },
          ]}
        />
      ),
      patterns,
      onPatterns: setPatterns,
    },
    side: (width: number) =>
      (beside(width) ? "right" : "below") as "right" | "below",
    chartWidth: (width: number) =>
      beside(width) ? width - LEGEND_SPACE : width,
  };
}
