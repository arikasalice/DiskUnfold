import { useMemo } from "react";
import type { AnalysisClient, LocaleBundleBody } from "../../ipc";
import type { LocaleSettingsProps } from "./LocaleSettings";
import { translationIncomplete } from "./translation-coverage";
import { useSizeUnit } from "./use-size-unit";

/**
 * The optional inputs of the appearance and language settings that come from
 * outside the language profile: whether the selected language is complete and
 * the saved size unit.
 *
 * `bundles` is `undefined` until the language bundles are loaded. Where
 * `analysisEnabled` is false the display preferences are off, so no size unit
 * is requested. The result holds only the fields that apply, so it can be
 * spread into the props.
 */
export function useLocaleSettingsExtras(
  bundles:
    { selected: LocaleBundleBody; baseline: LocaleBundleBody } | undefined,
  client: AnalysisClient,
  analysisEnabled: boolean,
): Pick<LocaleSettingsProps, "translationIncomplete" | "sizeUnit"> {
  const incomplete = useMemo(
    () => bundles && translationIncomplete(bundles.selected, bundles.baseline),
    [bundles],
  );
  const sizeUnit = useSizeUnit(analysisEnabled ? client : undefined);
  return {
    ...(incomplete && { translationIncomplete: true }),
    ...(sizeUnit && { sizeUnit }),
  };
}
