import type { LegendEntry } from "../components";
import type { VizModel } from "../ipc";
import { FOLDER_CLASS, coloredByClass, legendLook } from "./legend-style";
import { legendClassName } from "./model-text";
import type { VizText } from "./model-text";

/**
 * The class column of a row: the class name from the model's legend; empty for a
 * folder, which has no class (README 44.2); without a model, the key's own words.
 */
export function classCell(
  model: VizModel | undefined,
  key: string,
  text: VizText,
) {
  if (key === FOLDER_CLASS) return "";
  if (!model) return text.key(key);
  return model.legend.some((entry) => entry.key === key)
    ? legendClassName(model, key, text)
    : text.key(key);
}

/**
 * The legend items of a model coloured by class, in the engine's order. A
 * folder is never an item; none at all for a model without class colours.
 */
export function legendEntries(model: VizModel, text: VizText): LegendEntry[] {
  if (!coloredByClass(model)) return [];
  const order = model.legend.map((entry) => entry.key);
  return model.legend
    .filter((entry) => entry.key !== FOLDER_CLASS)
    .map((entry) => ({
      key: entry.key,
      label: legendClassName(model, entry.key, text),
      count: text.quantity(entry.itemCount),
      total: text.quantity(entry.total.quantity),
      ...legendLook(entry.key, order),
    }));
}

/**
 * Whether the chart shows a folder that is not expanded: its items are not in
 * the legend, so the legend totals can be less than the scope total (README 44.2).
 */
export function hasClosedFolders(model: VizModel) {
  const parents = new Set(
    model.shapes.flatMap((shape) => shape.parentShapeRef ?? []),
  );
  return model.shapes.some(
    (shape) =>
      shape.legendClassKey === FOLDER_CLASS &&
      !(shape.shapeRef && parents.has(shape.shapeRef)),
  );
}
