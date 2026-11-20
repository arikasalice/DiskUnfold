import type { ChartPattern } from "../components";
import { PLACE_PATTERNS } from "../components";
import type { VizModel } from "../ipc";

/** How a legend class is drawn: its palette place or neutral, and its pattern. */
export type LegendLook = {
  tone: string;
  pattern: ChartPattern | null;
  /** Drawn even when patterns are off (the neutrals, the ninth class onward). */
  patternAlways: boolean;
};

export const TYPE_CRITERION = "type";
export const FOLDER_CLASS = "type.folder";
const TYPE_UNAVAILABLE = "unclassified";
const NO_EXTENSION = "type.no_extension";

/** Fixed places of the types (F06 README 4.2, 44.3): a type looks the same everywhere. */
const TYPE_PLACES: Readonly<Record<string, number>> = {
  "type.document": 1,
  "type.audio": 2,
  "type.image": 3,
  "type.installer_image": 4,
  "type.video": 5,
  "type.archive": 7,
  "type.unclassified": 8,
};

/** Whether the model's shapes take their legend classes' colours. */
export function coloredByClass(model: Pick<VizModel, "representation">) {
  return (
    model.representation.colorCriterion === TYPE_CRITERION &&
    model.representation.format !== "histogram"
  );
}

/**
 * The look of a legend class. A folder has a look of its own and no legend
 * entry. A class the screen does not know takes the place of its position in
 * the legend; from the ninth on the places repeat with the next pattern.
 */
export function legendLook(key: string, order: readonly string[]): LegendLook {
  if (key === FOLDER_CLASS)
    return { tone: "folder", pattern: null, patternAlways: false };
  if (key === TYPE_UNAVAILABLE)
    return { tone: "neutral", pattern: "dense-hatch", patternAlways: true };
  if (key === NO_EXTENSION)
    return { tone: "neutral-2", pattern: "sparse-dots", patternAlways: true };
  const fixed = Object.hasOwn(TYPE_PLACES, key) ? TYPE_PLACES[key] : undefined;
  const position = Math.max(0, order.indexOf(key));
  const place = fixed ?? (position % 8) + 1;
  if (fixed === undefined && position >= 8)
    return {
      tone: String(place),
      pattern: PLACE_PATTERNS[place % 8] ?? null,
      patternAlways: true,
    };
  return {
    tone: String(place),
    pattern: PLACE_PATTERNS[place - 1] ?? null,
    patternAlways: false,
  };
}
