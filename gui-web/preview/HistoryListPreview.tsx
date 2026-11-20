import { HistoryPanel } from "../src/features/history/HistoryPanel";
import { LocaleTextContext } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import { regional } from "./BrowseScanStatusPreview";
import { historyListClient } from "./history-list-fixture";
import { koreanText } from "./result-summary-fixture";

/**
 * The product history list and detail views on fixed Korean scope-root
 * records, in the regional format the app gives them (times in UTC).
 */
export function HistoryListPreview() {
  return (
    <LocaleTextContext.Provider value={koreanText}>
      <RegionalFormatContext.Provider value={regional}>
        <HistoryPanel client={historyListClient} onClose={() => undefined} />
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}
