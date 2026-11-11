import type { VisualizationMessageKey } from "./messages";
import type {
  SizeValue,
  VizModel,
  VizQuantity,
  VizRow,
  VizShape,
} from "../../ipc";

/** Localization belongs to the caller; unknown keys must retain a visible limit. */
export type VizText = {
  message: (key: VisualizationMessageKey) => string;
  key: (key: string) => string;
  quantity: (value: VizQuantity) => string;
  size: (value: SizeValue) => string;
  observedAt: (value: string | null) => string;
};

/** Raw byte evidence remains readable even when the main value uses rounded units. */
export function rawSourceSize(size: SizeValue): string {
  const evidence = {
    measured: "Measured",
    os_reported: "Reported by the file system",
    estimated: "Estimated",
    unknown: "Evidence unavailable",
  }[size.evidence];
  const value =
    size.state === "unknown"
      ? "Unavailable"
      : `${size.state === "lower_bound" ? "At least " : ""}${size.bytes} bytes`;
  return `${value} · ${evidence}`;
}

export function shapeLabels(
  model: VizModel,
  shape: VizShape,
  row: VizRow | undefined,
  text: VizText,
) {
  const legend = model.legend.find(
    (entry) => entry.key === shape.legendClassKey,
  );
  const denominator = row?.denominator ?? model.denominator;
  const name = row?.displayName ?? "Item name not yet retrieved";
  const value = shape.quantity
    ? text.quantity(shape.quantity)
    : text.size(shape.size);
  const accessibleLabel = [
    name,
    value,
    row?.ratio ?? "Ratio unavailable",
    text.quantity(denominator.value),
    text.key(denominator.basis),
    text.key(legend?.labelKey ?? shape.legendClassKey),
    ...shape.displayFlags.map(text.key),
    text.key(model.origin),
  ].join(" · ");
  return {
    accessibleLabel,
    displayLabel:
      model.representation.labelMode === "name_only"
        ? name
        : `${name} · ${value}`,
  };
}
