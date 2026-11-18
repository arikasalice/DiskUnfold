import { useMemo } from "react";
import { useLocaleText } from "../i18n/presentation";
import type { LocaleText } from "../i18n/presentation";
import { visualizationMessages } from "./messages";
import type { VizText } from "./model-text";

/** Binds the message function to the active locale's messages. */
export function bindVizMessage(localeText: LocaleText): VizText["message"] {
  return (key, values) =>
    localeText(
      `ui.visualization.${key}`,
      visualizationMessages.en[key],
      values,
    );
}

/**
 * Binds the message function of `text` to the active locale and keeps the
 * other formatters of the caller. The result is stable while `text` and the
 * locale are.
 */
export function useLocalizedVizText(text: VizText): VizText {
  const localeText = useLocaleText();
  return useMemo<VizText>(
    () => ({ ...text, message: bindVizMessage(localeText) }),
    [localeText, text],
  );
}
