import { LocaleTextContext } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import type { RegionalFormat } from "../src/i18n/regional-format";
import { TargetPicker } from "../src/features/analysis/TargetPicker";
import type { useTargets } from "../src/state/analysis-targets";
import type { AnalysisTarget, TargetProbeEvent } from "../src/ipc";
import { unixPath } from "../src/ipc";
import { ACCESS_NOT_CHECKED } from "../src/features/analysis/target-access";
import { englishText, koreanText } from "./result-summary-fixture";
import "../src/theme/drive-picker.css";
import "../src/theme/analysis-workspace.css";

const regional: RegionalFormat = {
  language: "ko",
  decimalSeparator: ".",
  groupingSeparator: ",",
  groupingSize: 3,
  datePattern: "yyyy.MM.dd HH:mm:ss",
  dateFormat: "lang_default",
  groupNumbers: true,
  timeZonePolicy: "local",
  localTimeZoneStatus: "identified",
};
const observedAt = "2026-11-21T09:00:00.000Z";
const GiB = 1024n ** 3n;

const target = (
  raw: string,
  extra: Partial<AnalysisTarget> = {},
): AnalysisTarget => ({
  kind: "internal",
  path: unixPath(raw),
  connected: true,
  volumeId: null,
  identitySource: "unavailable",
  access: "checking",
  reasonKey: null,
  ...extra,
});

const system = [
  "/Library/Developer/CoreSimulator/Cryptex/Images/bundle/SimRuntimeBundle-1045155A",
  "/Library/Developer/CoreSimulator/Volumes/iOS_22C150",
  "/Library/Developer/CoreSimulator/Volumes/iOS_23D8133",
  "/System/Volumes/Hardware/iSCPreboot",
  "/System/Volumes/Preboot",
  "/System/Volumes/Update",
  "/System/Volumes/VM",
  "/System/Volumes/xarts",
  "/private/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/C/com.apple.appstoreagent",
];
const targets = [
  target("/"),
  target("/Volumes/home", {
    kind: "network_mount",
    access: "unknown",
    reasonKey: ACCESS_NOT_CHECKED,
  }),
  ...system.map((raw) => target(raw, { defaultVisible: false })),
];
const probes: Record<string, TargetProbeEvent> = {
  [unixPath("/").raw]: {
    sequence: 1,
    target: unixPath("/"),
    access: "accessible",
    reasonKey: null,
    totalBytes: String(1800n * GiB),
    availableBytes: String(104n * GiB),
    capacityReason: null,
    aggregationUncertain: false,
    observedAt,
  },
  [unixPath("/Volumes/home").raw]: {
    sequence: 2,
    target: unixPath("/Volumes/home"),
    access: "unknown",
    reasonKey: ACCESS_NOT_CHECKED,
    totalBytes: String(14336n * GiB),
    availableBytes: String(8100n * GiB),
    capacityReason: null,
    aggregationUncertain: true,
    observedAt,
  },
};
const list = {
  targets,
  probeJobRef: null,
  observedAt,
  unavailableReason: null,
};
const picker = {
  list,
  probes,
  loading: false,
  error: undefined,
  finished: true,
  refresh: () => Promise.resolve(),
  cancel: () => undefined,
} as unknown as ReturnType<typeof useTargets>;

/** The real location picker with fixed volumes; no engine calls are made. */
export function TargetPickerPreview() {
  const params = new URLSearchParams(window.location.search);
  const scene = params.get("scene") ?? "default";
  const localeText = params.get("locale") === "en" ? englishText : koreanText;
  const text: LocaleText = (key, fallback, values) =>
    localeText(
      key,
      fallback,
      Object.fromEntries(
        Object.entries(values ?? {}).map(([name, value]) => [
          name,
          String(value),
        ]),
      ),
    );
  const checking = scene === "checking";
  return (
    <LocaleTextContext.Provider value={text}>
      <RegionalFormatContext.Provider value={regional}>
        <div className="du-target-picker-preview">
          <TargetPicker
            targets={picker}
            disabled={false}
            selected={[checking ? unixPath("/Volumes/home") : undefined]}
            choose={() => undefined}
            addPath={() => undefined}
            removePath={() => undefined}
            dropEnabled={false}
            dropError={undefined}
            pathDescription=""
            reviewDescriptionId={undefined}
            feedback={() => null}
            isInitial={false}
            onPathCompositionChange={() => undefined}
            review={{
              reviewing: false,
              awaiting: checking,
              targets: undefined,
              openPrivacySettings: () => undefined,
            }}
          />
        </div>
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}
