import { HistoryPanel } from "../src/features/history/HistoryPanel";
import { LocaleTextContext } from "../src/i18n/presentation";
import { historyListClient } from "./history-list-fixture";
import { koreanText } from "./result-summary-fixture";

/** The product history list and detail views on fixed Korean scope-root records. */
export function HistoryListPreview() {
  return (
    <LocaleTextContext.Provider value={koreanText}>
      <HistoryPanel client={historyListClient} onClose={() => undefined} />
    </LocaleTextContext.Provider>
  );
}
