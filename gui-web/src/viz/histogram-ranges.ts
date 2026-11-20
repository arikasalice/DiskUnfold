import type { VizModel, VizRow } from "../ipc";
import type { HistogramRange } from "../components";
import { rowTargetKey } from "./model-adapter";
import { binItems } from "./model-text";
import type { VizText } from "./model-text";
import { rowName } from "./row-text";

/**
 * The intervals of a histogram from its table rows, in model order, so the
 * chart keeps the place of an empty one (F06 README 43). An interval is named
 * "{range}, {n} items, {size}" whichever axis is drawn; undefined for another
 * format.
 */
export function histogramRanges(
  model: VizModel,
  rows: readonly VizRow[],
  text: VizText,
): HistogramRange[] | undefined {
  if (model.representation.format !== "histogram") return undefined;
  return rows.flatMap((row) => {
    const rowRef = rowTargetKey(row);
    if (row.itemRef !== null || rowRef === null) return [];
    const name = rowName(row, model, text);
    const size = text.size(row.size);
    return [
      {
        rowRef,
        name,
        value: row.quantity ? text.quantity(row.quantity) : size,
        accessibleLabel: row.memberCount
          ? `${name}, ${binItems(row.memberCount, text)}, ${size}`
          : `${name}, ${size}`,
      },
    ];
  });
}
