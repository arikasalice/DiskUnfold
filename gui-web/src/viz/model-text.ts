import type { VisualizationMessageKey } from "./messages";
import { rowName } from "./row-text";
import { interpolate } from "../i18n/presentation";
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
 * Where a shape sits in a depth 2 treemap: an expanded folder holds `childCount`
 * drawn shapes, and a shape inside one has that folder's row (`undefined` while
 * it has not arrived). A top-level shape that holds nothing has no place.
 */
export type ShapePlace = {
  childCount?: number;
  parentRow?: VizRow | undefined;
  inFolder?: boolean;
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
  );
  // Every shape is named alike: name, value, share of the scope, its flags
  // (F06 README 10, 35); no internal marks such as class or origin.
  const accessibleLabel = [placed, ...shape.displayFlags.map(text.key)].join(
    ", ",
  );
  return {
    accessibleLabel,
    ...(place.childCount ? { container: true } : {}),
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
 * The accessible name of an expanded folder ("{name}, folder, …, {count} shown
 * inside") or of a shape inside one ("{name}, in {folder}, …"), F06 design README
 * section 27; `null` for a top-level shape that holds nothing.
 */
function placedLabel(
  place: ShapePlace,
  values: Record<"name" | "value" | "ratio" | "denominator" | "folder", string>,
  text: VizText,
): string {
  if (place.childCount)
    return vizMessage(text, "shapeFolder", {
      ...values,
      count: BigInt(place.childCount),
    });
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
