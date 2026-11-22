import type { VisualizationMessageKey } from "./messages";
import { rowName } from "./row-text";
import { interpolate } from "../i18n/presentation";
import { FOLDER_CLASS, coloredByClass, legendLook } from "./legend-style";
import { folderTypes } from "./type-slices";
import { sankeyLinkWords } from "./sankey-text";
import type {
  SizeValue,
  VizModel,
  VizQuantity,
  VizRow,
  VizShape,
} from "../ipc";

/** Localization belongs to the caller; unknown keys must retain a visible limit. */
export type VizText = {
  message: (
    key: VisualizationMessageKey,
    values?: Readonly<Record<string, string | bigint>>,
  ) => string;
  key: (key: string) => string;
  quantity: (value: VizQuantity) => string;
  size: (value: SizeValue) => string;
  observedAt: (value: string | null) => string;
  /**
   * Words a reason key the collector reported (`scan.*`). Optional: without it
   * such a key gets the general "reason unavailable" phrase.
   */
  scanReason?: (key: string) => string;
  /**
   * Words a canonical ratio string as a percentage. Optional: without it the
   * ratio is shown as the engine wrote it.
   */
  ratio?: (value: string) => string;
};

/** Applies placeholders even for injected text functions that only accept keys. */
export function vizMessage(
  text: Pick<VizText, "message">,
  key: VisualizationMessageKey,
  values?: Readonly<Record<string, string | bigint>>,
): string {
  return formatTemplate(text.message(key, values), values);
}

function formatTemplate(
  template: string,
  values?: Readonly<Record<string, string | bigint>>,
): string {
  return interpolate(template, (name) => String(values?.[name] ?? `{${name}}`));
}

/** Raw byte evidence remains readable even when the main value uses rounded units. */
export function rawSourceSize(
  size: SizeValue,
  message: VizText["message"],
): string {
  const format = (
    key: VisualizationMessageKey,
    values?: Readonly<Record<string, string | bigint>>,
  ) => formatTemplate(message(key, values), values);
  const evidence = {
    measured: format("measuredEvidence"),
    os_reported: format("reportedEvidence"),
    estimated: format("estimatedEvidence"),
    unknown: format("unavailableEvidence"),
  }[size.evidence];
  const value =
    size.state === "unknown"
      ? format("unavailableValue")
      : size.state === "lower_bound"
        ? format("rawBytesAtLeast", { count: size.bytes })
        : format("rawBytes", { count: size.bytes });
  return `${value} · ${evidence}`;
}

/**
 * The info card says the same as the shape's name, a fact a line (README 8):
 * the value, the share and what it is a share of, the class, the flags.
 */
function shapeCard(
  shape: VizShape,
  text: VizText,
  facts: {
    name: string;
    value: string;
    share: string | null;
    said: readonly string[];
    binCount: VizQuantity | undefined;
  },
) {
  return {
    title: facts.name,
    lines: [
      facts.binCount
        ? `${binItems(facts.binCount, text)}, ${text.size(shape.size)}`
        : cardValue(shape, facts.value, text),
      ...(facts.share ? [facts.share] : []),
      ...facts.said,
      ...(shape.displayFlags.length
        ? [shape.displayFlags.map(text.key).join(", ")]
        : []),
    ],
  };
}

/**
 * What a histogram interval says about its list (F06 README 52): with a list
 * condition and items, its name ends in "View in list" and its card says it opens
 * the list; without a condition (a size basis the list cannot filter) the card says
 * so and the name adds nothing. An empty interval opens nothing and says nothing.
 */
function intervalWords(
  binCount: VizShape["memberCount"],
  row: VizRow | undefined,
  text: VizText,
): { said: string[]; card: string[] } {
  const empty = binCount?.state === "exact" && binCount.value === "0";
  if (!binCount || !row?.groupRef || empty) return { said: [], card: [] };
  return row.condition
    ? {
        said: [text.message("intervalShowInList")],
        card: [text.message("intervalCardOpens")],
      }
    : { said: [], card: [text.message("intervalCardNoList")] };
}

/** A card's value line: the shown value, and the raw bytes when they are known (README 8). */
function cardValue(shape: VizShape, value: string, text: VizText) {
  const size = shape.size;
  if (shape.quantity || size.state === "unknown") return value;
  const raw = vizMessage(
    text,
    size.state === "lower_bound" ? "rawBytesAtLeast" : "rawBytes",
    { count: size.bytes },
  );
  return `${value} (${raw})`;
}

/**
 * Where a shape sits in a depth 2 treemap: an expanded folder holds `childCount`
 * drawn shapes, and a shape inside one has that folder's row (`undefined` while
 * it has not arrived). A top-level shape that holds nothing has no place.
 */
export type ShapePlace = {
  childCount?: number;
  parentRow?: VizRow | undefined;
  inFolder?: boolean;
  /** The bands into a Sankey type node: the row they come from and their bytes. */
  incoming?: readonly { row: VizRow | undefined; quantity: VizQuantity }[];
};

export function shapeLabels(
  model: VizModel,
  shape: VizShape,
  row: VizRow | undefined,
  text: VizText,
  place: ShapePlace = {},
) {
  const denominator = row?.denominator ?? model.denominator;
  const name = row
    ? rowName(row, model, text)
    : text.message("itemNamePending");
  const value = shape.quantity
    ? text.quantity(shape.quantity)
    : text.size(shape.size);
  const ratio = row
    ? rowRatioText(row, text)
    : text.message("ratioUnavailable");
  const placed = placedLabel(
    place,
    {
      name,
      value,
      ratio,
      denominator: `${text.key(denominator.basis)} ${text.quantity(denominator.value)}`,
      folder: place.parentRow
        ? rowName(place.parentRow, model, text)
        : text.message("itemNamePending"),
    },
    text,
    shape.itemKind === "folder",
  );
  // Every shape is named alike: name, value, share of the scope, its class when
  // the chart is coloured by class (FR6), its flags (F06 README 10, 35); no
  // internal marks such as origin.
  // Without its row the share is unknown and is left out (README 35).
  // A histogram interval says its item count and its size whichever axis is
  // drawn, so changing the axis does not change its name (README 43).
  const binCount =
    model.representation.format === "histogram" ? shape.memberCount : undefined;
  const look = classLook(model, shape, place, text);
  const interval = intervalWords(binCount, row, text);
  const types = closedFolderTypes(model, shape, place, text);
  const node = sankeyNode(
    { model, shape, row, name, ratio, place, classSaid: look.said },
    text,
  );
  const said = [...node.classSaid, ...types.said];
  const accessibleLabel = [
    binCount
      ? `${name}, ${binItems(binCount, text)}, ${text.size(shape.size)}`
      : row
        ? placed
        : `${name}, ${value}`,
    ...said,
    ...shape.displayFlags.map(text.key),
    ...interval.said,
  ].join(", ");
  const card = shapeCard(shape, text, {
    name,
    value,
    share:
      row && !binCount
        ? vizMessage(text, "cardShare", {
            ratio,
            denominator: `${text.key(denominator.basis)} ${text.quantity(denominator.value)}`,
          })
        : null,
    said: [...said, ...interval.card, ...node.links],
    binCount,
  });
  return {
    accessibleLabel,
    card,
    ...node.drawn,
    ...look.drawn,
    ...types.drawn,
    ...(place.childCount ? { container: true } : {}),
    ...shapeMarks(shape),
    // A lower bound is drawn with a dashed right and bottom edge: it may be larger (4.6).
    ...((shape.quantity ?? shape.size).state === "lower_bound"
      ? { lowerBound: true }
      : {}),
    // A shape whose row has not arrived is named for assistive technology only;
    // the drawing carries no label rather than a placeholder.
    displayLabel: !row
      ? ""
      : model.representation.labelMode === "name_only"
        ? name
        : `${name} · ${value}`,
    ...(row
      ? {
          displayName: name,
          ...(model.representation.labelMode === "name_only"
            ? {}
            : { displayValue: value }),
        }
      : {}),
  };
}

/**
 * What a folder drawn closed says and draws of its files by type (README 54); one
 * drawn open shows them as its children.
 */
function closedFolderTypes(
  model: VizModel,
  shape: VizShape,
  place: ShapePlace,
  text: VizText,
) {
  const types = place.childCount
    ? null
    : folderTypes(
        shape.typeSlices,
        shape.size,
        model.legend.map((entry) => entry.key),
        {
          message: text.message,
          quantity: text.quantity,
          className: (key) => legendClassName(model, key, text),
        },
      );
  if (!types) return { said: [] as string[], drawn: {} };
  return {
    said: types.said,
    drawn: {
      slices: types.bar,
      ...(types.total === undefined ? {} : { slicesTotal: types.total }),
    },
  };
}

/**
 * What a Sankey node adds (README 12.1): its links under a "Linked items" line
 * in its card, as the table lists them, and its share of the scope drawn after
 * its value. Nothing for any other chart or a node whose row has not arrived.
 */
function sankeyNode(
  node: {
    model: VizModel;
    shape: VizShape;
    row: VizRow | undefined;
    name: string;
    ratio: string;
    place: ShapePlace;
    classSaid: string[];
  },
  text: VizText,
): { links: string[]; drawn: { displayShare?: string }; classSaid: string[] } {
  const { model, row, name, ratio, classSaid } = node;
  if (model.representation.format !== "sankey" || !row)
    return { links: [], drawn: {}, classSaid };
  const drawn = row.ratio ? { displayShare: ratio } : {};
  // A type node is its class: it names the folders that send to it instead of
  // repeating its class (design, 2026-11-23).
  if (node.shape.column === "type")
    return {
      links: incomingWords(model, node.place.incoming ?? [], text),
      drawn,
      classSaid: [],
    };
  const words = sankeyLinkWords(model, row, name, {
    ...text,
    className: (key) => legendClassName(model, key, text),
  });
  return {
    links: words.length ? [text.message("linkedItems"), ...words] : [],
    drawn,
    classSaid,
  };
}

/** Folders that send to a type node shown at once; the rest are summed. */
const INCOMING_SHOWN = 5;

/**
 * "Media에서, 96 MiB" for the five largest folders sending to a type node, then
 * "나머지 폴더 3개 · 4 MiB" for the rest; nothing without any.
 */
function incomingWords(
  model: VizModel,
  incoming: NonNullable<ShapePlace["incoming"]>,
  text: VizText,
): string[] {
  const amount = (value: VizQuantity) =>
    "value" in value ? BigInt(value.value) : 0n;
  const sorted = [...incoming].sort((a, b) => {
    const difference = amount(b.quantity) - amount(a.quantity);
    return difference > 0n ? 1 : difference < 0n ? -1 : 0;
  });
  if (sorted.length === 0) return [];
  const shown = sorted.slice(0, INCOMING_SHOWN).map(({ row, quantity }) =>
    vizMessage(text, "sankeyLinkFrom", {
      from: row ? rowName(row, model, text) : text.message("itemNamePending"),
      value: text.quantity(quantity),
    }),
  );
  const rest = sorted.slice(INCOMING_SHOWN);
  const restWords = rest.length
    ? [
        vizMessage(text, "sankeyLinkRest", {
          count: BigInt(rest.length),
          size: text.quantity({
            unit: "bytes",
            state: "exact",
            value: String(
              rest.reduce((sum, { quantity }) => sum + amount(quantity), 0n),
            ),
          }),
        }),
      ]
    : [];
  return [text.message("linkedItems"), ...shown, ...restWords];
}

/**
 * The accessible name of an expanded folder ("{name}, folder, …, {count} shown
 * inside") or of a shape inside one ("{name}, in {folder}, …"), F06 design README
 * section 27; `null` for a top-level shape that holds nothing.
 */
function placedLabel(
  place: ShapePlace,
  values: Record<"name" | "value" | "ratio" | "denominator" | "folder", string>,
  text: VizText,
  folder: boolean,
): string {
  if (place.childCount)
    return vizMessage(text, "shapeFolder", {
      ...values,
      count: BigInt(place.childCount),
    });
  // A folder drawn closed, such as a bar, still says it is a folder (#201).
  if (folder)
    return vizMessage(
      text,
      place.inFolder ? "shapeClosedFolderInFolder" : "shapeClosedFolder",
      values,
    );
  return vizMessage(
    text,
    place.inFolder ? "shapeInFolder" : "shapeTop",
    values,
  );
}
/**
 * The ratio of a table row in words. Only a row whose value is exact has one; a
 * missing value or a lower bound makes the ratio unavailable too.
 */
export function rowRatioText(
  row: Pick<VizRow, "ratio" | "size" | "quantity">,
  text: VizText,
): string {
  const exact = row.quantity
    ? row.quantity.state === "exact"
    : row.size.state === "exact";
  return row.ratio && exact
    ? (text.ratio?.(row.ratio) ?? row.ratio)
    : text.message("unavailableValue");
}

/** "6 items" for an interval's exact item count; any other count as its quantity. */
export function binItems(count: VizQuantity, text: VizText) {
  return count.unit === "items" &&
    count.state === "exact" &&
    /^[0-9]+$/.test(count.value)
    ? text.message("binItems", { count: count.value })
    : text.quantity(count);
}

/**
 * The name of a legend class: its label key in the active language, or the
 * general "unknown class" words for a label the screen does not know.
 */
export function legendClassName(
  model: Pick<VizModel, "legend">,
  key: string,
  text: VizText,
) {
  const labelKey = model.legend.find((entry) => entry.key === key)?.labelKey;
  const name = labelKey === undefined ? undefined : text.key(labelKey);
  return name === undefined || name === text.message("unrecognizedValue")
    ? text.message("legend.unknownClass")
    : name;
}

/**
 * How a shape shows its legend class when the chart is coloured by class: the
 * look it is drawn with, and the class name said with it. An expanded folder
 * keeps its frame; a folder has no class to say.
 */
function classLook(
  model: VizModel,
  shape: VizShape,
  place: ShapePlace,
  text: VizText,
) {
  if (!coloredByClass(model) || place.childCount)
    return { drawn: {}, said: [] as string[] };
  const key = shape.legendClassKey;
  const look = legendLook(
    key,
    model.legend.map((entry) => entry.key),
  );
  return {
    drawn: { ...look, legendClass: key },
    said: key === FOLDER_CLASS ? [] : [legendClassName(model, key, text)],
  };
}

/**
 * A shape's kind marks: a folder carries the folder icon in a bar chart (#318);
 * a group is a group with a dashed border in every state, since it stands for
 * many (README 4.6, 47.4).
 */
function shapeMarks(shape: VizShape) {
  if (!shape.groupRef)
    return shape.itemKind === "folder" ? { folder: true } : {};
  return { group: true, dashed: true };
}
