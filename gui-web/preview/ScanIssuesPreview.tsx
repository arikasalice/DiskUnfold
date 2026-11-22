import { ScanIssues } from "../src/features/analysis/ScanIssues";
import { LocaleTextContext } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import type {
  AnalysisClient,
  ScanFailures,
  ScanFailuresRequest,
} from "../src/ipc";
import { regional } from "./BrowseScanStatusPreview";
import { koreanText } from "./result-summary-fixture";
import "../src/theme/analysis-results.css";

const version = "0193f000-0000-7000-8000-000000000358";

const rows: ScanFailures["rows"] = [
  {
    kind: "unread_range",
    stage: "open_directory",
    pathRef: { rootIndex: 0, relative: [] },
    reasonKey: "scan.path_denied",
    displayPath: "C:\\Users\\민서\\문서\\사진\\2025 휴가",
  },
  {
    kind: "unread_range",
    stage: "enumerate",
    pathRef: { rootIndex: 0, relative: [] },
    reasonKey: "scan.path_denied",
    displayPath: "C:\\Users\\민서\\문서\\사진\\스캔본",
  },
];

/** Fixed answers; nothing reaches an engine. */
const client = {
  call: (async (_name: string, body: ScanFailuresRequest) =>
    ({
      resultVersion: body.resultVersion,
      rows,
      totalCount: rows.length,
    }) satisfies ScanFailures) as AnalysisClient["call"],
  subscribe: () => () => undefined,
} satisfies AnalysisClient;

/**
 * The product's scan issue list restricted to one storage and one folder, so both chips
 * stand side by side (F02 #358). The list opens by the same request Browse sends.
 */
export function ScanIssuesPreview() {
  return (
    <LocaleTextContext.Provider value={koreanText}>
      <RegionalFormatContext.Provider value={regional}>
        <div className="du-analysis du-analysis--workspace">
          <ScanIssues
            client={client}
            resultVersion={version}
            onReopenLatest={() => undefined}
            laneRequest={{
              requestId: 1,
              rootIndexes: [0],
              storageLabel: "C:",
              storagePath: "C:\\Users\\민서\\문서",
              storagePaths: ["C:\\Users\\민서\\문서"],
              allStorages: false,
              folder: {
                item: { resultVersion: version, itemNo: 4 },
                label: "사진",
                path: "C:\\Users\\민서\\문서\\사진",
              },
            }}
          />
        </div>
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}
