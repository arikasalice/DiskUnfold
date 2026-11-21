import { createContext, useContext } from "react";
import { formatByteUnit } from "../../i18n/bytes";
import type { SizeDisplay } from "../../i18n/bytes";
import type { LocaleText } from "../../i18n/presentation";
import { useLocaleOsFamily } from "../../state/use-locale";
import { defaultSizeDisplay } from "../../state/size-display";
import { SIZE_UNIT_RULES } from "../../ipc";
import type { HistoryItemRow } from "../../ipc";

/** The user's size display for history screens, when the app supplies it. */
export const HistorySizeDisplay = createContext<SizeDisplay | undefined>(
  undefined,
);

export function useHistorySizeDisplay() {
  const display = useContext(HistorySizeDisplay);
  const osFamily = useLocaleOsFamily();
  return display ?? defaultSizeDisplay(osFamily);
}

/**
 * Shows a recorded item size with the shared size rule. A lower bound reads
 * "At least", and a missing or unknown size is never shown as zero.
 */
export function observedSizeText(
  result: NonNullable<HistoryItemRow["result"]>,
  display: SizeDisplay,
  text: LocaleText,
): string {
  const lowerBound = result.observedSizeState === "lower_bound";
  if (
    result.observedSize === null ||
    (!lowerBound && result.observedSizeState !== "exact")
  )
    return text("ui.history.size_unknown", "Cannot be confirmed");
  const size = formatByteUnit(
    BigInt(result.observedSize),
    SIZE_UNIT_RULES[display.system],
    SIZE_UNIT_RULES.decimalPlaces,
    display.unit,
    lowerBound,
    display.regional,
  );
  return lowerBound
    ? text("ui.history.size_at_least", "At least {size}", { size })
    : size;
}
