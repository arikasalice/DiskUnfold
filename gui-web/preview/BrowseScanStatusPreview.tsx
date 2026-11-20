import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import { LocaleTextContext } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import type { RegionalFormat } from "../src/i18n/regional-format";
import { browsePort, browseVersion } from "./browse-scan-status-fixture";
import { koreanText } from "./result-summary-fixture";

/** The Korean defaults of the regional format, as the app builds them, with times in UTC. */
export const regional: RegionalFormat = {
  decimalSeparator: ".",
  groupingSeparator: ",",
  groupingSize: 3,
  datePattern: "yyyy.MM.dd HH:mm:ss",
  dateFormat: "lang_default",
  groupNumbers: true,
  timeZonePolicy: "utc",
  localTimeZoneStatus: "identified",
  byteUnitText: koreanText("ui.unit.byte", "bytes"),
};

/**
 * The product's browse panel on a fixed folder of a finished scan, in
 * Korean, with every mark of the "Scan status" column and blank rows.
 */
export function BrowseScanStatusPreview() {
  const notice = new URLSearchParams(window.location.search).has("notice");
  return (
    <LocaleTextContext.Provider value={koreanText}>
      <RegionalFormatContext.Provider value={regional}>
        <div className="du-gallery-browse">
          <BrowsePanel
            port={browsePort(notice)}
            resultVersion={browseVersion}
            display={{ system: "iec", unit: "auto", regional }}
          />
        </div>
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}
