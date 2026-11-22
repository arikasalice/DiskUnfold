import type { SupportOsFamily } from "../ipc";
import type { SizeDisplay } from "../i18n/bytes";

/**
 * Resolves the file-manager convention for an automatic display. Older hosts
 * may omit the family, so unknown and omitted values retain the IEC fallback.
 */
export function defaultSizeDisplay(osFamily?: SupportOsFamily): SizeDisplay {
  return {
    system: osFamily === "macos" || osFamily === "linux" ? "si" : "iec",
    unit: "auto",
  };
}
