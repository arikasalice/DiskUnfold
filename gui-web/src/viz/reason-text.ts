import { visualizationReason } from "./messages";
import type { VizText } from "./model-text";

/**
 * Words one reason key for the screen. A `scan.*` key is a reason the collector
 * reported and is worded by the scan reason table the caller supplies (without
 * one it gets the general phrase). Every other key goes through the viz reason
 * table, which also words the unwatched freshness reason. A key neither table
 * knows gets the general phrase, never the key.
 */
export function explainReason(
  key: string,
  text: Pick<VizText, "message" | "scanReason">,
): string {
  if (key.startsWith("scan.") && text.scanReason) return text.scanReason(key);
  return visualizationReason(key, text.message);
}
