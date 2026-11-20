import { useState } from "react";
import { SegmentedChoice } from "../components";
import type { VizCatalog } from "../ipc";
import type { VisualizationViewRequest, VizText } from "../viz";

/** The histogram's item-count measure; every other format and axis measures size. */
const COUNT = "count";
const SIZE = "size";

/**
 * The vertical axis of the histogram (F06 README 43): item count or size.
 *
 * It starts on the item count, which answers the histogram's question; the size
 * totals are what the bar chart and the treemap show. The choice is kept while
 * the screen is used and every folder shares it; a restart starts on the item
 * count again. It is offered only for a histogram whose catalog serves the
 * count measure; otherwise the owner's measure is asked for.
 */
export function useHistogramAxis(
  format: string,
  catalog: VizCatalog | undefined,
  owner: { request: Pick<VisualizationViewRequest, "measure"> } | undefined,
  text: VizText,
) {
  const [axis, setAxis] = useState(COUNT);
  const ownerMeasure = owner?.request.measure ?? SIZE;
  if (format !== "histogram" || !catalog?.measures.includes(COUNT))
    return { measure: ownerMeasure, control: undefined };
  return {
    measure: axis === COUNT ? COUNT : ownerMeasure,
    // Changing the axis keeps the selection and the focus and says nothing.
    control: (
      <SegmentedChoice
        label={text.message("axisChoice")}
        value={axis}
        onChange={setAxis}
        options={[
          { value: COUNT, label: text.message("measureCount") },
          { value: SIZE, label: text.message("measureSize") },
        ]}
      />
    ),
  };
}
