import type { VisualizationMessageKey } from "./messages";
import { interpolate } from "../../i18n/presentation";
import type {
  SizeValue,
  VizModel,
  VizQuantity,
  VizRow,
  VizShape,
} from "../../ipc";

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
  return interpolate(template, (name) =>
    String(values?.[name] ?? `{${name}}`),
  );
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
        ? format("lowerBound", {
            value: format("byteCount", { count: size.bytes }),
          })
        : format("byteCount", { count: size.bytes });
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
  const name = row?.displayName ?? text.message("itemNamePending");
  const value = shape.quantity
    ? text.quantity(shape.quantity)
    : text.size(shape.size);
  const accessibleLabel = [
    name,
    value,
    row?.ratio ?? text.message("ratioUnavailable"),
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
