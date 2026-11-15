import { useMemo } from "react";
import { SIZE_UNIT_RULES } from "../ipc";
import type { SizeValue, VizQuantity } from "../ipc";
import { formatByteUnit } from "../i18n/bytes";
import type { SizeDisplay } from "../i18n/bytes";
import { useLocaleText } from "../i18n/presentation";
import { displayTimestamp } from "../i18n/regional-format";
import { createVizKeyText } from "./key-text";
import { bindVizMessage } from "./use-viz-text";
import type { VizText } from "./model-text";

const WHOLE_NUMBER = /^[0-9]+$/;

/**
 * Builds the text a product view needs from the active-language messages and the
 * size display the user chose. Sizes keep their lower bounds and unknown values;
 * a value the formatter cannot read becomes the general phrase, never a guess.
 */
export function createProductionVizText(
  message: VizText["message"],
  display: SizeDisplay,
): VizText {
  const unrecognized = () => message("unrecognizedValue");
  const bytes = (value: string, lowerBound: boolean) => {
    if (!WHOLE_NUMBER.test(value)) return unrecognized();
    const formatted = formatByteUnit(
      BigInt(value),
      SIZE_UNIT_RULES[display.system],
      SIZE_UNIT_RULES.decimalPlaces,
      display.unit,
      lowerBound,
      display.regional,
    );
    return lowerBound
      ? message("sizeAtLeast", { value: formatted })
      : formatted;
  };
  const size = (value: SizeValue) =>
    value.state === "unknown"
      ? message("unavailableValue")
      : bytes(value.bytes, value.state === "lower_bound");
  const quantity = (value: VizQuantity) => {
    if (value.state === "unknown") return message("unavailableValue");
    const lowerBound = value.state === "lower_bound";
    if (value.unit === "bytes") return bytes(value.value, lowerBound);
    if (value.unit !== "items" || !WHOLE_NUMBER.test(value.value))
      return unrecognized();
    const count = message("itemCount", { count: value.value });
    return lowerBound ? message("sizeAtLeast", { value: count }) : count;
  };
  const observedAt = (value: string | null) => {
    if (value === null) return message("observedAtUnknown");
    const shown = display.regional
      ? displayTimestamp(Date.parse(value), display.regional)
      : undefined;
    return shown?.text ?? value;
  };
  return {
    message,
    key: createVizKeyText(message),
    quantity,
    size,
    observedAt,
  };
}

/** The product text for the active language; stable while the language and display are. */
export function useProductionVizText(display: SizeDisplay): VizText {
  const localeText = useLocaleText();
  return useMemo(
    () => createProductionVizText(bindVizMessage(localeText), display),
    [localeText, display],
  );
}
