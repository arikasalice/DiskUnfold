import { useMemo } from "react";
import { useLocaleText } from "../i18n/presentation";
import { visualizationMessages } from "./messages";
import type { VizText } from "./model-text";

/**
 * Binds the message function of `text` to the active locale and keeps the
 * other formatters of the caller. The result is stable while `text` and the
 * locale are.
 */
export function useLocalizedVizText(text: VizText): VizText {
  const localeText = useLocaleText();
  return useMemo<VizText>(
    () => ({
      ...text,
      message: (key, values) =>
        localeText(
          `ui.visualization.${key}`,
          visualizationMessages.en[key],
          values,
        ),
    }),
    [localeText, text],
  );
}
