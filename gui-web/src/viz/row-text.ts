import type { VizModel, VizQuantity, VizRow } from "../ipc";
import type { VisualizationMessageKey } from "./messages";

/** The one thing a row name needs from the text: a localized message. */
type MessageText = {
  message: (
    key: VisualizationMessageKey,
    values?: Readonly<Record<string, string | bigint>>,
  ) => string;
  /** Names a label key the screen knows; optional for callers without one. */
  key?: (key: string) => string;
};

/** A key such as "ui.visualization.group.other": dotted words without spaces. */
const KEY_SHAPE = /^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z0-9_]+)+$/;
const WHOLE_NUMBER = /^[0-9]+$/;
/** Label keys of engine-made groups that the bundle names. */
const OTHER_GROUP_LABEL = "ui.visualization.group.other";

type NamedRow = Pick<
  VizRow,
  "itemRef" | "groupRef" | "displayName" | "labelKey" | "memberCount"
>;

/**
 * The name a row carries as text, or `null` when the row has none to show.
 *
 * An item row always has its own name, and a `labelKey` on it is never read: the
 * contract rejects one and the screen does not trust it either. A group row has
 * a name only when the supplier wrote one (for example a histogram interval): a
 * row with a label key, an empty name, or a name shaped like a message key has
 * none.
 */
export function plainRowName(row: NamedRow): string | null {
  if (row.itemRef !== null || row.groupRef === null) return row.displayName;
  if (row.labelKey !== undefined) return null;
  const name = row.displayName.trim();
  return name === "" || KEY_SHAPE.test(name) ? null : row.displayName;
}

/**
 * The name to show for a row of `model`. A group row whose label key is the
 * registered "other" key reads "Other (n)" when its member count is exact,
 * "Other (n+)" when it is a lower bound, and "Other items" when it is unknown.
 * A row from before member counts uses the model's grouped item count, and only
 * when the model holds exactly one group. An unregistered or missing key never
 * reaches the screen: it gets the general phrase "Other items".
 */
export function rowName(
  row: NamedRow,
  model: Pick<VizModel, "shapes" | "summary">,
  text: MessageText,
): string {
  const plain = plainRowName(row);
  if (plain !== null) return plain;
  if (row.labelKey === OTHER_GROUP_LABEL && row.groupRef !== null)
    return row.memberCount === undefined
      ? otherGroupName(row.groupRef, model, text)
      : countedName(row.memberCount, text);
  // A group named by a key the screen knows, such as a Sankey type node by its
  // type's label (#296); an unknown key still gets the general phrase.
  if (
    row.labelKey?.startsWith("ui.visualization.") &&
    row.groupRef !== null &&
    text.key
  ) {
    const named = text.key(row.labelKey);
    if (named !== text.message("unrecognizedValue")) return named;
  }
  return text.message("group.other_unknown");
}

function otherGroupName(
  groupRef: string,
  model: Pick<VizModel, "shapes" | "summary">,
  text: MessageText,
): string {
  // Only drawn group shapes are counted, on purpose: the row list is read in
  // pages, so the rows alone cannot show that this is the model's only group, and
  // summary.groupedItems sums every group. A group row without a group shape
  // therefore gets no count until rows carry their own member count.
  const groups = new Set(
    model.shapes
      .filter((shape) => shape.itemRef === null && shape.groupRef !== null)
      .map((shape) => shape.groupRef),
  );
  const grouped = model.summary.groupedItems;
  return groups.size === 1 && groups.has(groupRef)
    ? countedName(grouped, text)
    : text.message("group.other_unknown");
}

/** "Other (n)" for an exact item count, "Other (n+)" for a lower bound, else "Other items". */
function countedName(count: VizQuantity, text: MessageText): string {
  if (
    count.unit === "items" &&
    count.state !== "unknown" &&
    WHOLE_NUMBER.test(count.value)
  )
    return text.message(
      count.state === "exact" ? "group.other_count" : "group.other_at_least",
      { count: count.value },
    );
  return text.message("group.other_unknown");
}
